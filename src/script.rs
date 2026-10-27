use crate::format::Node;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    process::{Command, Stdio},
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Argument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choice_hints: Vec<Option<String>>,
    pub name: String,
    #[serde(default, rename = "enum", skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

pub fn choices(node: &Node) -> Result<Vec<Choice>> {
    let Node::Array(values) = node else {
        anyhow::bail!("values must be a list");
    };
    let choices = values
        .iter()
        .map(|value| match value {
            Node::String(value) => Ok(Choice {
                value: value.clone(),
                hint: None,
            }),
            Node::Table(table) => {
                ensure!(
                    table
                        .keys()
                        .all(|key| matches!(key.as_str(), "value" | "hint")),
                    "Choices accept only value and hint"
                );
                Ok(Choice {
                    value: table
                        .get("value")
                        .and_then(Node::as_str)
                        .context("Choice requires a string value")?
                        .to_owned(),
                    hint: table
                        .get("hint")
                        .map(|node| {
                            node.as_str()
                                .context("Choice hint must be a string")
                                .map(str::to_owned)
                        })
                        .transpose()?,
                })
            }
            _ => anyhow::bail!("Choices must be strings or value/hint objects"),
        })
        .collect::<Result<Vec<_>>>()?;
    validate_choices(&choices)?;
    Ok(choices)
}

pub fn validate_choices(choices: &[Choice]) -> Result<()> {
    ensure!(!choices.is_empty(), "List must have at least one choice");
    let mut seen = BTreeSet::new();
    for choice in choices {
        ensure!(!choice.value.contains('\0'), "Choices cannot contain NUL");
        ensure!(
            seen.insert(&choice.value),
            "Duplicate choice: {}",
            choice.value
        );
    }
    Ok(())
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
                    let mut reference = None;
                    let choices = table
                        .get("enum")
                        .map(|value| {
                            if let Node::Table(table) = value {
                                ensure!(table.len() == 1, "Enum reference accepts only ref");
                                let name = table
                                    .get("ref")
                                    .and_then(Node::as_str)
                                    .context("Enum reference requires a string ref")?;
                                crate::template::expression(name)?;
                                reference = Some(name.to_owned());
                                return Ok(Vec::new());
                            }
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
                    Ok(Argument {
                        name,
                        choices: if reference.is_some() { None } else { choices },
                        reference,
                        ..Default::default()
                    })
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
        validate_arguments(self.args.as_deref())?;
        for (key, value) in &self.env {
            ensure!(
                crate::template::valid_name(key),
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
        prompt: impl FnMut(&Argument) -> Result<String>,
    ) -> Result<Vec<String>> {
        self.validate()?;
        resolve_arguments(self.args.as_deref(), supplied, prompt)
    }

    pub fn run(&self, arguments: &[String]) -> Result<i32> {
        let arguments = self.resolve_arguments(arguments, |arg| {
            anyhow::bail!("Missing argument: {}", arg.name)
        })?;
        let mut command = self.command(&arguments);
        let status = command
            .status()
            .with_context(|| format!("Cannot run {} in {}", self.shell, self.cwd.display()))?;
        use std::os::unix::process::ExitStatusExt;
        Ok(status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(1)))
    }

    fn command(&self, arguments: &[String]) -> Command {
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
        command
    }

    pub fn capture(&self, arguments: &[String]) -> Result<String> {
        let output = self
            .command(arguments)
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .output()
            .with_context(|| format!("Cannot run {} in {}", self.shell, self.cwd.display()))?;
        ensure!(
            output.status.success(),
            "Referenced script failed: {}",
            output.status
        );
        let mut value = String::from_utf8(output.stdout).context("Script output must be UTF-8")?;
        ensure!(!value.contains('\0'), "Script output cannot contain NUL");
        while value.ends_with('\n') {
            value.pop();
            if value.ends_with('\r') {
                value.pop();
            }
        }
        Ok(value)
    }
}

impl Argument {
    pub fn validate_value(&self, value: &str) -> Result<()> {
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

pub fn validate_arguments(args: Option<&[Argument]>) -> Result<()> {
    if let Some(args) = args {
        let mut names = BTreeSet::new();
        for arg in args {
            ensure!(
                crate::template::valid_name(&arg.name),
                "Invalid argument name: {}",
                arg.name
            );
            ensure!(
                names.insert(&arg.name),
                "Duplicate argument name: {}",
                arg.name
            );
            if let Some(reference) = &arg.reference {
                ensure!(
                    arg.choices.is_none(),
                    "Argument cannot combine a reference and literal choices"
                );
                crate::template::expression(reference)?;
            }
            ensure!(
                arg.choice_hints.is_empty()
                    || arg
                        .choices
                        .as_ref()
                        .is_some_and(|choices| choices.len() == arg.choice_hints.len()),
                "Choice hints must match the choices"
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
    Ok(())
}

pub fn resolve_arguments(
    args: Option<&[Argument]>,
    supplied: &[String],
    mut prompt: impl FnMut(&Argument) -> Result<String>,
) -> Result<Vec<String>> {
    validate_arguments(args)?;
    let Some(args) = args else {
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
