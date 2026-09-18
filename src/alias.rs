use crate::{
    shell::Shell,
    store::{Source, State},
};
use anyhow::{Result, ensure};
use std::{collections::BTreeSet, path::Path};

pub fn valid_name(name: &str) -> Result<()> {
    ensure!(
        name.starts_with(|c: char| c.is_ascii_alphabetic())
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
        "Alias names must start with a letter and contain only letters, digits, - or _"
    );
    ensure!(
        ![
            "qrlkit",
            "cd",
            "pwd",
            "echo",
            "test",
            "type",
            "alias",
            "unalias",
            "source",
            "exec",
            "exit",
            "return",
            "eval",
            "set",
            "export",
            "function",
            "if",
            "then",
            "else",
            "fi",
            "for",
            "while",
            "do",
            "done",
            "read",
            "command",
            "builtin",
            "help",
            "true",
            "false",
            "case",
            "esac",
            "in",
            "until",
            "select",
            "coproc",
            "time",
            "end",
            "switch",
            "elif",
            "break",
            "continue",
            "not",
            "and",
            "or",
            "begin",
            "foreach",
            "repeat",
            "nocorrect",
            "noglob"
        ]
        .contains(&name.to_ascii_lowercase().as_str()),
        "Alias conflicts with shell command: {name}"
    );
    Ok(())
}

pub fn validate(sources: &[Source]) -> Result<()> {
    let mut names = BTreeSet::new();
    for name in sources.iter().filter_map(|s| s.alias.as_ref()) {
        valid_name(name)?;
        ensure!(
            names.insert(name.to_ascii_lowercase()),
            "Duplicate alias: {name}"
        );
        if let Some(path) = std::env::var_os("PATH") {
            let suffixes = if cfg!(windows) {
                vec!["", ".exe", ".cmd", ".bat", ".com"]
            } else {
                vec![""]
            };
            ensure!(
                !std::env::split_paths(&path).any(|dir| suffixes
                    .iter()
                    .any(|ext| dir.join(format!("{name}{ext}")).is_file())),
                "Alias conflicts with an existing command: {name}"
            );
        }
    }
    Ok(())
}

pub fn quote(value: &str, shell: &Shell) -> String {
    match shell {
        Shell::Powershell => format!("'{}'", value.replace('\'', "''")),
        Shell::Fish => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
        _ => format!("'{}'", value.replace('\'', "'\\''")),
    }
}

pub fn functions(state: &State, config: &Path, shell: Shell) -> Result<String> {
    let config = quote(&std::path::absolute(config)?.to_string_lossy(), &shell);
    let mut result = String::new();
    // Explicit aliases remain supported and take precedence over automatic names.
    let mut commands = Vec::new();
    let mut names = BTreeSet::new();
    for source in &state.sources {
        if let Some(name) = &source.alias {
            valid_name(name)?;
            names.insert(name.to_ascii_lowercase());
            commands.push((name.clone(), source, None));
        }
    }
    for source in &state.sources {
        for root in crate::import::roots(source) {
            if valid_name(&root).is_err() || !names.insert(root.to_ascii_lowercase()) {
                eprintln!("qrlkit: cannot create command {root:?}; use qrlkit {root} instead");
                continue;
            }
            commands.push((root.clone(), source, Some(root)));
        }
    }
    for (name, source, root) in commands {
        let path = quote(&source.path.to_string_lossy(), &shell);
        let root = root
            .map(|root| format!(" --root {}", quote(&root, &shell)))
            .unwrap_or_default();
        let command = format!("qrlkit --config {config} --source {path}{root} __lookup");
        // Check at shell startup too, where user-defined functions/aliases are visible.
        result.push_str(&match shell {
            Shell::Bash | Shell::Zsh => format!("if ! command -v {name} >/dev/null 2>&1; then\n{name}() {{ {command} \"$@\"; }}\nelse\nprintf '%s\\n' 'qrlkit: alias {name} conflicts with an existing command' >&2\nfi\n"),
            Shell::Fish => format!("if not type -q {name}\nfunction {name}\n{command} $argv\nend\nelse\necho 'qrlkit: alias {name} conflicts with an existing command' >&2\nend\n"),
            Shell::Powershell => format!("if (-not (Get-Command '{name}' -ErrorAction SilentlyContinue)) {{\nfunction global:{name} {{ {command} @args }}\n}} else {{ Write-Warning 'qrlkit: alias {name} conflicts with an existing command' }}\n"),
        });
    }
    Ok(result)
}
