use crate::{
    shell::{self, Shell},
    store::State,
};
use anyhow::{Context, Result, bail, ensure};
use clap::ValueEnum;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const START: &str = "# >>> QRL shell integration >>>";
const END: &str = "# <<< QRL shell integration <<<";

fn detect() -> Result<Shell> {
    let value = std::env::var("SHELL").unwrap_or_default();
    match Path::new(&value).file_name().and_then(|s| s.to_str()) {
        Some("bash") => Ok(Shell::Bash),
        Some("zsh") => Ok(Shell::Zsh),
        Some("fish") => Ok(Shell::Fish),
        Some("pwsh" | "powershell") => Ok(Shell::Powershell),
        _ if cfg!(windows) => Ok(Shell::Powershell),
        _ => bail!(
            "Cannot detect a supported shell from SHELL; use qrlkit init --help for manual integration"
        ),
    }
}

fn startup(shell: &Shell) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot locate home directory")?;
    let env_path = |key| {
        std::env::var_os(key)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    };
    Ok(match shell {
        Shell::Zsh => env_path("ZDOTDIR").unwrap_or(home).join(".zshrc"),
        Shell::Bash if cfg!(target_os = "macos") => [".bash_profile", ".bash_login", ".profile"]
            .iter()
            .map(|name| home.join(name))
            .find(|path| path.exists())
            .unwrap_or_else(|| home.join(".bash_profile")),
        Shell::Bash => home.join(".bashrc"),
        Shell::Fish => env_path("XDG_CONFIG_HOME")
            .unwrap_or_else(|| home.join(".config"))
            .join("fish/config.fish"),
        Shell::Powershell => {
            let mut result = None;
            for executable in ["pwsh", "powershell"] {
                if let Ok(output) = std::process::Command::new(executable)
                    .args(["-NoLogo", "-NoProfile", "-Command", "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); $PROFILE.CurrentUserAllHosts"])
                    .output()
                    && output.status.success()
                {
                        let path = PathBuf::from(String::from_utf8(output.stdout)?.trim());
                        if path.is_absolute() { result = Some(path); break; }
                }
            }
            result.context(
                "Cannot locate a PowerShell profile; install PowerShell or use qrlkit init powershell",
            )?
        }
    })
}

fn configured_text(original: &str, shell: Shell) -> Result<String> {
    let manual = match shell {
        Shell::Bash => "eval \"$(qrlkit init bash)\"",
        Shell::Zsh => "eval \"$(qrlkit init zsh)\"",
        Shell::Fish => "qrlkit init fish | source",
        Shell::Powershell => "qrlkit init powershell | Out-String | Invoke-Expression",
    };
    if !original.contains(START)
        && (original.lines().any(|line| line.trim() == manual)
            || original.contains(shell::init(shell.clone())))
    {
        return Ok(original.into());
    }
    let block = format!("{START}\n{}{END}\n", shell::init(shell));
    let starts: Vec<_> = original.match_indices(START).collect();
    let ends: Vec<_> = original.match_indices(END).collect();
    ensure!(
        starts.len() == ends.len() && starts.len() <= 1,
        "Malformed QRL markers; startup file left unchanged"
    );
    if let (Some((start, _)), Some((end, _))) = (starts.first(), ends.first()) {
        ensure!(
            start < end,
            "Malformed QRL markers; startup file left unchanged"
        );
        let after = end + END.len();
        let after = after + usize::from(original[after..].starts_with('\n'));
        return Ok(format!(
            "{}{}{}",
            &original[..*start],
            block,
            &original[after..]
        ));
    }
    Ok(format!(
        "{}{}{}",
        original,
        if original.is_empty() || original.ends_with('\n') {
            ""
        } else {
            "\n"
        },
        block
    ))
}

fn install(path: &Path, shell: Shell) -> Result<bool> {
    install_with(path, |original| configured_text(original, shell))
}

fn install_with(path: &Path, transform: impl FnOnce(&str) -> Result<String>) -> Result<bool> {
    let path = if fs::symlink_metadata(path).is_ok() {
        fs::canonicalize(path)?
    } else {
        path.to_path_buf()
    };
    let original = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).context("Cannot read shell startup file"),
    };
    let updated = transform(original.as_deref().unwrap_or(""))?;
    if original.as_deref() == Some(&updated) {
        return Ok(false);
    }
    let parent = path
        .parent()
        .context("Startup file has no parent directory")?;
    fs::create_dir_all(parent)?;
    if let Some(text) = &original {
        // Keep a recovery copy of the original; never overwrite an earlier backup.
        let mut backup = path.as_os_str().to_os_string();
        backup.push(".qrl-backup");
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(PathBuf::from(backup))
        {
            Ok(mut file) => {
                file.set_permissions(fs::metadata(&path)?.permissions())?;
                file.write_all(text.as_bytes())?;
                file.sync_all()?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e).context("Cannot back up shell startup file"),
        }
    }
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    if original.is_some() {
        file.as_file()
            .set_permissions(fs::metadata(&path)?.permissions())?;
    }
    file.write_all(updated.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(&path)?;
    Ok(true)
}

pub fn for_directories(state: &State) -> Result<()> {
    if std::env::var_os("QRL_CD_FILE").is_some() {
        return Ok(());
    }
    let has_directory = state
        .sources
        .iter()
        .flat_map(|source| &source.entries)
        .any(|entry| {
            entry.script.is_none()
                && !crate::resource::is_url(&entry.url)
                && Path::new(&entry.url).is_dir()
        });
    if !has_directory {
        return Ok(());
    }
    let shell = state.shell.clone().map(Ok).unwrap_or_else(detect)?;
    let path = startup(&shell)?;
    let changed = install(&path, shell)?;
    eprintln!(
        "Directory shell integration {} in {}. Open a new terminal to activate directory shortcuts.",
        if changed {
            "configured"
        } else {
            "already configured"
        },
        path.display()
    );
    Ok(())
}

/// Configure preferences only after all prompts have completed successfully.
pub fn interactive(state: &mut State, config: &Path) -> Result<()> {
    let browser = crate::browser::choose()?;
    let default_shell = detect().unwrap_or(if cfg!(windows) {
        Shell::Powershell
    } else {
        Shell::Bash
    });
    let title = format!(
        "Choose shell: bash, zsh, fish, powershell (empty for {})",
        default_shell.to_possible_value().unwrap().get_name()
    );
    let mut prompt = title.clone();
    let shell = loop {
        let value = crate::ui::input_optional(&prompt)?;
        if value.is_empty() {
            break default_shell;
        }
        match Shell::from_str(&value, true) {
            Ok(shell) => break shell,
            Err(_) => prompt = format!("Unsupported shell. {title}"),
        }
    };
    let filehook = choose_hook(
        "Filehook command (use file, e.g. nvim file; empty to print path)",
        crate::filehook::validate,
    )?;
    let dirhook = choose_hook(
        "Dirhook command (use dir, e.g. cd dir; empty to change directory)",
        crate::dirhook::validate,
    )?;
    let startup_path = startup(&shell)?;
    state.browser = Some(browser);
    state.shell = Some(shell.clone());
    state.filehook = filehook;
    state.dirhook = dirhook;
    state.save(config)?;
    install(&startup_path, shell)
        .context("Preferences saved, but shell integration failed; run qrlkit init to retry")?;
    for_aliases(state, config)
        .context("Preferences saved, but alias setup failed; run qrlkit init to retry")?;
    println!(
        "QRL configured. Shell integration installed in {}. Open a new terminal to activate it.",
        startup_path.display()
    );
    Ok(())
}

fn choose_hook(title: &str, validate: impl Fn(&str) -> Result<()>) -> Result<Option<String>> {
    let mut prompt = title.to_owned();
    loop {
        let value = crate::ui::input_optional(&prompt)?;
        if value.is_empty() {
            return Ok(None);
        }
        match validate(&value) {
            Ok(()) => return Ok(Some(value)),
            Err(error) => prompt = format!("{error}. {title}"),
        }
    }
}

/// Load aliases from live state on each new shell, so removals need no stale functions.
pub fn for_aliases(state: &State, config: &Path) -> Result<()> {
    if state.sources.is_empty() {
        return Ok(());
    }
    let shell = state.shell.clone().map(Ok).unwrap_or_else(detect)?;
    let path = startup(&shell)?;
    let config = crate::alias::quote(&std::path::absolute(config)?.to_string_lossy(), &shell);
    let loader = match shell {
        Shell::Bash => format!("eval \"$(qrlkit --config {config} __aliases bash)\""),
        Shell::Zsh => format!("eval \"$(qrlkit --config {config} __aliases zsh)\""),
        Shell::Fish => format!("qrlkit --config {config} __aliases fish | source"),
        Shell::Powershell => {
            format!(
                "qrlkit --config {config} __aliases powershell | Out-String | Invoke-Expression"
            )
        }
    };
    let changed = install_with(&path, |original| {
        // Upgrade the exact loader previously installed for this state file.
        let legacy_loader = loader.replacen("qrlkit --config", "qrl --config", 1);
        let original = original
            .split_inclusive('\n')
            .map(|line| {
                if line.trim_end_matches(['\r', '\n']) == legacy_loader {
                    line.replacen("qrl --config", "qrlkit --config", 1)
                } else {
                    line.to_owned()
                }
            })
            .collect::<String>();
        let updated = configured_text(&original, shell)?;
        if updated.lines().any(|line| line == loader) {
            return Ok(updated);
        }
        Ok(format!("{updated}\n{loader}\n"))
    })?;
    if changed {
        eprintln!(
            "QRL aliases configured in {}. Open a new terminal to activate them.",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_user_config_backs_up_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".zshrc");
        fs::write(&path, "export CUSTOM=yes").unwrap();
        assert!(install(&path, Shell::Zsh).unwrap());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("export CUSTOM=yes\n"));
        assert_eq!(content.matches(START).count(), 1);
        assert!(!install(&path, Shell::Zsh).unwrap());
        assert_eq!(
            fs::read_to_string(dir.path().join(".zshrc.qrl-backup")).unwrap(),
            "export CUSTOM=yes"
        );
    }
    #[test]
    fn malformed_markers_are_rejected_without_modification() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".bashrc");
        fs::write(&path, START).unwrap();
        assert!(install(&path, Shell::Bash).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), START);
    }
}
