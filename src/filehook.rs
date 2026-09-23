use anyhow::{Context, Result};
use std::path::Path;

pub fn validate(hook: &str) -> Result<()> {
    if hook.is_empty() {
        return Ok(());
    }
    crate::hook::expand(hook, "file", "file").map(|_| ())
}

pub fn run(hook: &str, path: &Path) -> Result<i32> {
    let expanded = crate::hook::expand(hook, "file", "\"$qrl_hook_file\"")?;
    crate::script::Script {
        body: format!("qrl_hook_file=$1\n{expanded}"),
        shell: "bash".into(),
        cwd: std::env::current_dir()?,
    }
    .run(&[path.to_str().context("File path is not UTF-8")?.into()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_operators_preserve_literal_strings_and_require_a_placeholder() {
        assert_eq!(
            crate::hook::expand("cat<file|sort > output && echo 'file'", "file", "PATH").unwrap(),
            "cat<PATH|sort > output && echo 'file'"
        );
        assert_eq!(
            crate::hook::expand("editor file; diff file otherfile", "file", "PATH").unwrap(),
            "editor PATH; diff PATH otherfile"
        );
        for invalid in [
            " ",
            "nvim",
            "nvim myfile",
            "nvim 'file'",
            "nvim 'file",
            "nvim file\n",
            "nvim file\\",
        ] {
            assert!(validate(invalid).is_err(), "{invalid}");
        }
        assert!(validate("").is_ok());
    }
}
