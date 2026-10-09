mod alias;
mod browser;
mod constants;
mod dirhook;
mod filehook;
mod format;
mod hook;
mod import;
mod resource;
mod script;
mod setup;
mod shell;
mod store;
mod template;
mod ui;

use anyhow::{Context, Result, ensure};
use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;
use store::State;

#[derive(Parser)]
#[command(
    version,
    about = "Locate websites, directories, and files from imported TOML, YAML, and JSON files"
)]
struct Cli {
    /// Override the default application state path
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[arg(long, hide = true)]
    source: Option<PathBuf>,
    #[arg(long, hide = true)]
    root: Option<String>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Register a config file or all TOML/YAML/JSON files directly in a directory
    Add {
        path: PathBuf,
        /// Save a collision strategy for newly imported files, including future reloads.
        /// When both files have overrides, the more recently added file wins.
        #[arg(long, value_enum)]
        collision_strategy: Option<store::CollisionStrategy>,
    },
    /// List registered config files and their effective namespaces
    Ls,
    /// Remove an imported file, including when it no longer exists
    Rm { path: PathBuf },
    /// Refresh all imports, preserving namespace renames
    Reload,
    /// Choose how shared namespaces are resolved (default: merge)
    SetCollisionStrategy { strategy: store::CollisionStrategy },
    /// Choose the default browser
    SetBrowser,
    /// Set the default file shell command; use an unquoted $file placeholder for the path
    SetFilehook {
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        command: Option<String>,
        /// Restore printing file paths
        #[arg(long)]
        clear: bool,
    },
    /// Set the default directory shell command; use an unquoted $dir placeholder for the path
    SetDirhook {
        #[arg(required_unless_present = "clear", conflicts_with = "clear")]
        command: Option<String>,
        /// Restore changing directory without running a hook
        #[arg(long)]
        clear: bool,
    },
    /// Delete saved imports, namespace renames, browser selection, and hooks
    Nuke,
    /// Run interactive setup, or print integration for an explicitly named shell
    Init { shell: Option<shell::Shell> },
    #[command(name = "__lookup", hide = true, disable_help_flag = true)]
    ScopedLookup {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        keys: Vec<String>,
    },
    #[command(name = "__aliases", hide = true)]
    Aliases { shell: shell::Shell },
    #[command(name = "__prompt", hide = true)]
    Prompt { request: PathBuf, response: PathBuf },
    #[command(external_subcommand)]
    Lookup(Vec<String>),
}

fn resolve(sources: &mut [store::Source], strategy: store::CollisionStrategy) -> Result<()> {
    alias::validate(sources)?;
    while let Some((a, b, root)) = import::collision(sources, strategy) {
        let index = if a == b {
            a
        } else {
            let choices = [a, b].map(|i| format!("{}: qrlkit {root}", sources[i].path.display()));
            [a, b][ui::select(
                "Import collision — choose the namespace to rename",
                &choices,
            )?]
        };
        let mut title = format!("Rename qrlkit {root}: enter a new name");
        loop {
            let alias = ui::input(&title)?;
            match import::rename(&mut sources[index], &root, &alias) {
                Ok(()) => break,
                Err(error) => title = error.to_string(),
            }
        }
    }
    Ok(())
}

/// Walk immediate children, keeping full-path lookup as the fast path.
fn browse(state: &State, mut key: Vec<String>) -> Result<Option<store::Entry>> {
    loop {
        let children = state.children(&key);
        if children.is_empty() && !key.is_empty() {
            return Ok(Some(state.lookup(&key)?.clone()));
        }
        if children.is_empty() {
            println!("No imports. Run qrlkit add <path>.\n");
            Cli::command().print_help()?;
            println!();
            return Ok(None);
        }
        let options = children
            .iter()
            .map(|child| {
                let mut full_key = key.clone();
                full_key.push(child.clone());
                let hint = state
                    .lookup(&full_key)
                    .ok()
                    .and_then(|entry| entry.hint.as_deref());
                ui::key_label(child, hint)
            })
            .collect::<Vec<_>>();
        let choice = ui::select_key(&options)?;
        key.push(children[choice].clone());
    }
}

fn run() -> Result<i32> {
    let cli = Cli::parse();
    let command = match cli.command.unwrap_or(Commands::Lookup(vec![])) {
        Commands::ScopedLookup { keys } => Commands::Lookup(keys),
        other => other,
    };
    ensure!(
        cli.source.is_none() || matches!(command, Commands::Lookup(_)),
        "Source scope is only available for resource lookup"
    );
    if let Commands::Prompt { request, response } = command {
        return ui::run_prompt(&request, &response).map(|_| 0);
    }
    if let Commands::Init { shell: Some(shell) } = command {
        print!("{}", shell::init(shell));
        return Ok(0);
    }
    let path = cli.config.unwrap_or(store::default_path()?);
    // Reset must work even when state is corrupt or browser setup is incomplete.
    if matches!(command, Commands::Nuke) {
        let shell_changed = setup::remove_integration()?;
        match std::fs::remove_file(&path) {
            Ok(()) => println!(
                "QRL state deleted{} Start again with qrlkit add <path>.",
                if shell_changed {
                    " and shell integration removed."
                } else {
                    "."
                }
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                println!(
                    "No QRL state to delete{} Start with qrlkit add <path>.",
                    if shell_changed {
                        "; shell integration removed."
                    } else {
                        "."
                    }
                );
            }
            Err(error) => {
                return Err(error).with_context(|| format!("Cannot delete {}", path.display()));
            }
        }
        return Ok(0);
    }
    let mut state = State::load(&path)?;
    if let Commands::Init { shell: None } = command {
        setup::interactive(&mut state, &path)?;
        return Ok(0);
    }
    if let Commands::Aliases { shell } = command {
        print!("{}", alias::functions(&state, &path, shell)?);
        return Ok(0);
    }
    // Keep removal available even if a registered source has disappeared or is invalid.
    if !matches!(
        command,
        Commands::Rm { .. }
            | Commands::Add { .. }
            | Commands::Reload
            | Commands::SetFilehook { .. }
            | Commands::SetDirhook { .. }
            | Commands::SetCollisionStrategy { .. }
    ) && !state.sources.is_empty()
    {
        let before = serde_yaml_ng::to_string(&state)?;
        state.sources = import::reload_with_constants(&state.sources, &constants::load(&path)?)?;
        resolve(&mut state.sources, state.collision_strategy)?;
        if serde_yaml_ng::to_string(&state)? != before {
            state.save(&path)?;
            setup::for_aliases(&state, &path)?;
            setup::for_directories(&state)
                .context("Reload saved, but directory shell setup failed")?;
        }
    }
    if let Commands::Lookup(mut key) = command {
        if let Some(root) = &cli.root {
            key.insert(0, root.clone());
        }
        let mut scoped;
        let lookup_state = if let Some(source) = &cli.source {
            let source = std::fs::canonicalize(source)?;
            scoped = state.clone();
            scoped.sources.retain(|s| s.path == source);
            ensure!(
                !scoped.sources.is_empty(),
                "Alias source is no longer registered"
            );
            // Scoped commands retain the names written in their own config.
            for source in &mut scoped.sources {
                if cli.root.is_some() {
                    continue;
                }
                for entry in &mut source.entries {
                    entry.key = import::map_key(&entry.key, &source.renames, true);
                }
            }
            &scoped
        } else {
            &state
        };
        let resource_prefix = (1..=key.len()).find(|&length| {
            lookup_state.lookup(&key[..length]).is_ok_and(|entry| {
                entry.script.is_some()
                    || template::names(&entry.url).is_ok_and(|names| !names.is_empty())
            })
        });
        let (key, mut arguments) = match resource_prefix {
            Some(length) => (key[..length].to_vec(), key[length..].to_vec()),
            None => (key, vec![]),
        };
        if arguments.first().is_some_and(|arg| arg == "--") {
            arguments.remove(0);
        }
        let Some(entry) = browse(lookup_state, key)? else {
            return Ok(0);
        };
        if let Some(script) = entry.script {
            let arguments = script.resolve_arguments(&arguments, |arg| {
                let title = format!("{}:", arg.name);
                match &arg.choices {
                    Some(choices) => {
                        let index = ui::select(&title, choices)?;
                        Ok(choices[index].clone())
                    }
                    None => ui::input(&title),
                }
            })?;
            eprintln!("qrlkit: running script..");
            return script.run(&arguments);
        }
        let value = template::expand(&entry.url, &arguments, |name| {
            ui::input(&format!("{name}:"))
        })?;
        match resource::resolve(&value)? {
            resource::Resource::Url(url) => {
                let override_browser = entry
                    .browser
                    .as_deref()
                    .filter(|value| !value.is_empty())
                    .map(browser::resolve);
                if override_browser.is_none() && state.browser.is_none() {
                    state.browser = Some(browser::choose()?);
                    state.save(&path)?;
                }
                eprintln!("qrlkit: open {url}");
                return browser::open(
                    override_browser
                        .as_ref()
                        .or(state.browser.as_ref())
                        .unwrap(),
                    &url,
                )
                .map(|_| 0);
            }
            resource::Resource::File(path) => {
                if let Some(hook) = entry.filehook.as_ref().or(state.filehook.as_ref())
                    && !hook.is_empty()
                {
                    return filehook::run(hook, &path);
                }
                println!("{}", path.display());
            }
            resource::Resource::Directory(path) => {
                let file = std::env::var_os("QRL_CD_FILE").context(
                    "Directory shortcuts need shell integration. Run qrlkit init --help for supported shells; for Bash/Zsh use eval \"$(qrlkit init zsh)\"")?;
                if let Some(hook) = entry.dirhook.as_ref().or(state.dirhook.as_ref())
                    && !hook.is_empty()
                {
                    let (status, directory) = dirhook::run(hook, &path)?;
                    if let Some(directory) = directory {
                        std::fs::write(file, format!("{}\n", directory.display()))?;
                    }
                    return Ok(status);
                }
                std::fs::write(file, format!("{}\n", path.display()))?;
                eprintln!("qrlkit: goto {}", path.display());
            }
        }
        return Ok(0);
    }
    match command {
        Commands::Add {
            path: source_path,
            collision_strategy,
        } => {
            let global = constants::load(&path)?;
            let directory = source_path.is_dir();
            let mut registered: std::collections::BTreeSet<_> =
                state.sources.iter().map(|s| s.path.clone()).collect();
            let mut additions = Vec::new();
            for candidate in import::paths(&source_path)? {
                let canonical = std::fs::canonicalize(&candidate)?;
                if !registered.insert(canonical) {
                    ensure!(directory, "File already imported; use qrlkit reload");
                    continue;
                }
                let mut source =
                    import::read_with_constants(&candidate, Default::default(), &global)?;
                source.collision_strategy = collision_strategy;
                additions.push(source);
            }
            if additions.is_empty() {
                println!(
                    "All config files in {} are already imported",
                    source_path.display()
                );
                return Ok(0);
            }
            let imported: Vec<_> = additions.iter().map(|s| s.path.clone()).collect();
            state.sources = import::reload_with_constants(&state.sources, &global)?;
            state.sources.extend(additions);
            resolve(&mut state.sources, state.collision_strategy)?;
            state.save(&path)?;
            for source in imported {
                println!("Imported {}", source.display());
            }
            setup::for_aliases(&state, &path)?;
            setup::for_directories(&state)
                .context("Import saved, but directory shell setup failed")?;
        }
        Commands::Reload => {
            state.sources =
                import::reload_with_constants(&state.sources, &constants::load(&path)?)?;
            resolve(&mut state.sources, state.collision_strategy)?;
            state.save(&path)?;
            println!("Reloaded {} file(s)", state.sources.len());
            setup::for_aliases(&state, &path)?;
            setup::for_directories(&state)
                .context("Reload saved, but directory shell setup failed")?;
        }
        Commands::Rm { path: source_path } => {
            let absolute = std::path::absolute(&source_path)?;
            let canonical = std::fs::canonicalize(&source_path).unwrap_or_else(|_| {
                absolute
                    .parent()
                    .and_then(|parent| std::fs::canonicalize(parent).ok())
                    .zip(absolute.file_name())
                    .map(|(parent, name)| parent.join(name))
                    .unwrap_or(absolute)
            });
            let index = state
                .sources
                .iter()
                .position(|s| s.path == canonical)
                .with_context(|| format!("File is not imported: {}", source_path.display()))?;
            state.sources.remove(index);
            state.save(&path)?;
            println!("Removed {}", source_path.display());
        }
        Commands::Ls => {
            if state.sources.is_empty() {
                println!("No imports. Run qrlkit add <path>.");
            }
            for source in &state.sources {
                println!(
                    "{} [{}]",
                    source.path.display(),
                    import::roots(source)
                        .into_iter()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        Commands::SetFilehook { command, .. } => {
            if let Some(hook) = &command {
                filehook::validate(hook)?;
            }
            state.filehook = command;
            state.save(&path)?;
            println!("File hook saved");
        }
        Commands::SetDirhook { command, .. } => {
            if let Some(hook) = &command {
                dirhook::validate(hook)?;
            }
            state.dirhook = command;
            state.save(&path)?;
            setup::for_aliases(&state, &path)
                .context("Directory hook saved, but shell setup failed; run qrlkit reload")?;
            println!(
                "Directory hook saved. Open a new terminal to activate updated shell integration."
            );
        }
        Commands::SetCollisionStrategy { strategy } => {
            state.sources =
                import::reload_with_constants(&state.sources, &constants::load(&path)?)?;
            resolve(&mut state.sources, strategy)?;
            state.collision_strategy = strategy;
            state.save(&path)?;
            setup::for_aliases(&state, &path)?;
            setup::for_directories(&state)
                .context("Collision strategy saved, but directory shell setup failed")?;
            println!(
                "Collision strategy set to {}",
                match strategy {
                    store::CollisionStrategy::Merge => "merge",
                    store::CollisionStrategy::Rename => "rename",
                }
            );
        }
        Commands::SetBrowser => {
            state.browser = Some(browser::choose()?);
            state.save(&path)?;
            println!("Browser set to {}", state.browser.as_ref().unwrap().name);
        }
        Commands::ScopedLookup { .. }
        | Commands::Aliases { .. }
        | Commands::Lookup(_)
        | Commands::Nuke
        | Commands::Init { .. }
        | Commands::Prompt { .. } => {
            unreachable!()
        }
    }
    Ok(0)
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("qrlkit: {error:#}");
            std::process::exit(1);
        }
    }
}
