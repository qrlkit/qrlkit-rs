use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

fn expand(hook: &str, replacement: &str) -> Result<String> {
    crate::hook::expand(hook, "dir", replacement)
}

pub fn validate(hook: &str) -> Result<()> {
    if hook.is_empty() {
        return Ok(());
    }
    expand(hook, "dir").map(|_| ())
}

pub fn run(hook: &str, path: &Path) -> Result<(i32, Option<PathBuf>)> {
    let directory_file = tempfile::NamedTempFile::new()?;
    let (shell, body) = if cfg!(windows) {
        let expanded = expand(hook, "$qrlHookDirectory")?;
        (
            "pwsh",
            format!(
                "$qrlHookDirectory = $args[0]\n$qrlHookResult = $args[1]\ntry {{\n{expanded}\n}} finally {{\n[System.IO.File]::WriteAllText($qrlHookResult, (Get-Location).Path)\n}}"
            ),
        )
    } else {
        let expanded = expand(hook, "\"$qrl_hook_directory\"")?;
        (
            "bash",
            format!(
                "qrl_hook_directory=$1\nqrl_hook_result=$2\ntrap 'builtin pwd -P > \"$qrl_hook_result\"' EXIT\n{expanded}"
            ),
        )
    };
    let status = crate::script::Script {
        body,
        shell: shell.into(),
        cwd: std::env::current_dir()?,
    }
    .run(&[
        path.to_str().context("Directory path is not UTF-8")?.into(),
        directory_file
            .path()
            .to_str()
            .context("Temporary path is not UTF-8")?
            .into(),
    ])?;
    let directory = std::fs::read_to_string(directory_file.path())?;
    if directory.is_empty() {
        return Ok((status, None));
    }
    let directory = directory.strip_suffix('\n').unwrap_or(&directory);
    ensure!(
        !directory.contains(['\n', '\r', '\0']),
        "Hook produced an unsupported directory path"
    );
    let directory = PathBuf::from(directory);
    ensure!(
        directory.is_absolute() && directory.is_dir(),
        "Hook produced an invalid directory path"
    );
    Ok((status, Some(directory)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unquoted_complete_placeholders_are_replaced() {
        assert_eq!(
            expand("cd dir&&printf '%s' 'dir'", "PATH").unwrap(),
            "cd PATH&&printf '%s' 'dir'"
        );
        assert_eq!(
            expand("echo directory; cd dir", "PATH").unwrap(),
            "echo directory; cd PATH"
        );
        for bad in [
            " ",
            "cd directory",
            "cd 'dir'",
            "cd dir\n",
            "cd dir\\",
            "cd dir && echo '",
        ] {
            assert!(validate(bad).is_err(), "{bad}");
        }
        assert!(validate("").is_ok());
    }
}
