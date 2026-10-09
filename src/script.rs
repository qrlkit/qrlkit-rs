use crate::format::Node;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    process::Command,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Script {
    pub body: String,
    pub shell: String,
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<Argument>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Argument {
    pub name: String,
    #[serde(default, rename = "enum", skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<String>>,
}

pub fn arguments(value: Option<&Node>) -> Result<Option<Vec<Argument>>> {
    value
        .map(|value| {
            let Node::Array(values) = value else {
                anyhow::bail!("args must be an array")
            };
            values
                .iter()
                .map(|value| {
                    let Node::Table(table) = value else {
                        anyhow::bail!("Each argument must be a table")
                    };
                    ensure!(
                        table
                            .keys()
                            .all(|key| matches!(key.as_str(), "name" | "enum")),
                        "Arguments accept only name and enum"
                    );
                    let name = table
                        .get("name")
                        .and_then(Node::as_str)
                        .context("Argument requires a string name")?
                        .to_owned();
                    let choices = table
                        .get("enum")
                        .map(|value| {
                            let Node::Array(values) = value else {
                                anyhow::bail!("Argument {name} requires an enum list")
                            };
                            values
                                .iter()
                                .map(|v| {
                                    v.as_str()
                                        .context("Enum values must be strings")
                                        .map(str::to_owned)
                                })
                                .collect::<Result<Vec<_>>>()
                        })
                        .transpose()?;
                    Ok(Argument { name, choices })
                })
                .collect()
        })
        .transpose()
}

pub fn environment(value: Option<&Node>) -> Result<BTreeMap<String, String>> {
    let Some(value) = value else {
        return Ok(BTreeMap::new());
    };
    let Node::Table(values) = value else {
        anyhow::bail!("env must be a table")
    };
    values
        .iter()
        .map(|(key, value)| {
            Ok((
                key.clone(),
                value
                    .as_str()
                    .context("Environment values must be strings")?
                    .to_owned(),
            ))
        })
        .collect()
}

impl Script {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.body.trim().is_empty() && !self.body.contains('\0'),
            "Script must be nonempty and contain no NUL"
        );
        ensure!(
            ["bash", "sh", "zsh"].contains(&self.shell.as_str()),
            "Unsupported shell {}; use bash, sh, or zsh",
            self.shell
        );
        ensure!(
            self.cwd.is_absolute(),
            "Script working directory must be absolute"
        );
        if let Some(args) = &self.args {
            let mut names = BTreeSet::new();
            for arg in args {
                ensure!(
                    crate::constants::valid_name(&arg.name),
                    "Invalid argument name: {}",
                    arg.name
                );
                ensure!(
                    names.insert(&arg.name),
                    "Duplicate argument name: {}",
                    arg.name
                );
                let Some(allowed) = &arg.choices else {
                    continue;
                };
                ensure!(
                    !allowed.is_empty(),
                    "Enum {} must have at least one choice",
                    arg.name
                );
                let mut choices = BTreeSet::new();
                for value in allowed {
                    ensure!(!value.contains('\0'), "Enum choices cannot contain NUL");
                    ensure!(
                        choices.insert(value),
                        "Duplicate choice for {}: {value}",
                        arg.name
                    );
                }
            }
        }
        for (key, value) in &self.env {
            ensure!(
                crate::constants::valid_name(key),
                "Invalid environment variable: {key}"
            );
            ensure!(
                key != "QRL_CD_FILE",
                "QRL_CD_FILE is reserved for shell integration"
            );
            ensure!(
                !value.contains('\0'),
                "Environment values cannot contain NUL"
            );
        }
        Ok(())
    }

    pub fn resolve_arguments(
        &self,
        supplied: &[String],
        mut prompt: impl FnMut(&Argument) -> Result<String>,
    ) -> Result<Vec<String>> {
        self.validate()?;
        let Some(args) = &self.args else {
            return Ok(supplied.to_vec());
        };
        ensure!(
            supplied.len() <= args.len(),
            "Expected {} argument(s), got {}",
            args.len(),
            supplied.len()
        );
        // Reject every supplied value before opening any prompt.
        for (arg, value) in args.iter().zip(supplied) {
            arg.validate_value(value)?;
        }
        let mut values = supplied.to_vec();
        for arg in args.iter().skip(values.len()) {
            let value = prompt(arg)?;
            arg.validate_value(&value)?;
            values.push(value);
        }
        Ok(values)
    }

    pub fn run(&self, arguments: &[String]) -> Result<i32> {
        let arguments = self.resolve_arguments(arguments, |arg| {
            anyhow::bail!("Missing argument: {}", arg.name)
        })?;
        let mut command = Command::new(&self.shell);
        match self.shell.as_str() {
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
        command.args(arguments).envs(&self.env);
        // A script may invoke QRL, but must not change the outer shell wrapper's directory.
        command.current_dir(&self.cwd).env_remove("QRL_CD_FILE");
        let status = command
            .status()
            .with_context(|| format!("Cannot run {} in {}", self.shell, self.cwd.display()))?;
        use std::os::unix::process::ExitStatusExt;
        Ok(status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)))
    }
}

impl Argument {
    fn validate_value(&self, value: &str) -> Result<()> {
        ensure!(!value.contains('\0'), "Arguments cannot contain NUL");
        let Some(choices) = &self.choices else {
            return Ok(());
        };
        ensure!(
            choices.iter().any(|choice| choice == value),
            "Invalid value {value:?} for {}; expected one of: {}",
            self.name,
            choices.join(", ")
        );
        Ok(())
    }
}
