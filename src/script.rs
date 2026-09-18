use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, process::Command};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Script {
    pub body: String,
    pub shell: String,
    pub cwd: PathBuf,
}

impl Script {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.body.trim().is_empty() && !self.body.contains('\0'),
            "Script must be nonempty and contain no NUL"
        );
        ensure!(
            ["bash", "sh", "zsh", "powershell", "pwsh"].contains(&self.shell.as_str()),
            "Unsupported $shell {}; use bash, sh, zsh, powershell, or pwsh",
            self.shell
        );
        ensure!(
            self.cwd.is_absolute(),
            "Script working directory must be absolute"
        );
        Ok(())
    }

    pub fn run(&self, arguments: &[String]) -> Result<i32> {
        self.validate()?;
        let mut command = Command::new(&self.shell);
        let mut script_file = None;
        match self.shell.as_str() {
            "powershell" | "pwsh" => {
                use std::io::Write;
                let mut file = tempfile::Builder::new().suffix(".ps1").tempfile()?;
                write!(
                    file,
                    "$ErrorActionPreference = 'Stop'\n$PSNativeCommandUseErrorActionPreference = $true\n{}\nif ($null -ne $LASTEXITCODE) {{ exit $LASTEXITCODE }}",
                    self.body
                )?;
                command
                    .args(["-NoLogo", "-NoProfile", "-File"])
                    .arg(file.path());
                script_file = Some(file);
            }
            "bash" | "zsh" => {
                command
                    .args(["-e", "-o", "pipefail", "-c"])
                    .arg(&self.body)
                    .arg("qrlkit");
            }
            _ => {
                command.args(["-e", "-c"]).arg(&self.body).arg("qrlkit");
            }
        }
        command.args(arguments);
        // A script may invoke QRL, but must not change the outer shell wrapper's directory.
        command.current_dir(&self.cwd).env_remove("QRL_CD_FILE");
        let status = command
            .status()
            .with_context(|| format!("Cannot run {} in {}", self.shell, self.cwd.display()))?;
        drop(script_file);
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            Ok(status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)))
        }
        #[cfg(not(unix))]
        {
            Ok(status.code().unwrap_or(1))
        }
    }
}
