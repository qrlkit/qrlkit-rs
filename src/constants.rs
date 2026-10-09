//! File-local constants and the optional TOML settings beside the state file.
use crate::format::Node;
use anyhow::{Context, Result, bail, ensure};
use std::{collections::BTreeMap, path::Path};

pub type Constants = BTreeMap<String, Node>;

pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn definitions(node: Node) -> Result<Constants> {
    let Node::Table(values) = node else {
        bail!("constants must be a table");
    };
    for (name, value) in &values {
        ensure!(valid_name(name), "Invalid constant name: {name}");
        let validate = |value: &Node| -> Result<()> {
            let value = value
                .as_str()
                .context("Constants must be strings or lists of strings")?;
            ensure!(
                !value.contains(['{', '}', '\0']),
                "Constant {name} cannot contain braces or NUL; nested references are unsupported"
            );
            Ok(())
        };
        match value {
            Node::Array(values) => {
                for value in values {
                    validate(value)?;
                }
            }
            value => validate(value)?,
        }
    }
    Ok(values)
}

pub fn load(state_path: &Path) -> Result<Constants> {
    let path = state_path.with_file_name("constants.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error).with_context(|| format!("Cannot read {}", path.display())),
    };
    let Node::Table(mut root) = crate::format::parse(&path, &text)? else {
        unreachable!()
    };
    let values = root
        .remove("constants")
        .context("Global constants.toml requires [constants]")?;
    ensure!(
        root.is_empty(),
        "Global constants.toml accepts only [constants]"
    );
    definitions(values).with_context(|| format!("Invalid constants in {}", path.display()))
}

fn lookup<'a>(reference: &str, local: &'a Constants, global: &'a Constants) -> Result<&'a Node> {
    let (namespace, name) = reference
        .split_once('.')
        .context("Expected constants.name or global.name")?;
    ensure!(valid_name(name), "Invalid constant reference: {reference}");
    let values = match namespace {
        "constants" => local,
        "global" => global,
        _ => bail!("Expected constants.name or global.name: {reference}"),
    };
    values
        .get(name)
        .with_context(|| format!("Unknown constant: {reference}"))
}

fn expand(value: &str, local: &Constants, global: &Constants) -> Result<String> {
    let mut result = String::new();
    let mut rest = value;
    while let Some(start) = rest.find('{') {
        result.push_str(&rest[..start]);
        rest = &rest[start..];
        if rest.starts_with("{constants.") || rest.starts_with("{global.") {
            let end = rest.find('}').context("Unclosed constant reference")?;
            let reference = &rest[1..end];
            result.push_str(
                lookup(reference, local, global)?
                    .as_str()
                    .with_context(|| format!("Constant {reference} must be a string here"))?,
            );
            rest = &rest[end + 1..];
        } else {
            result.push('{');
            rest = &rest[1..];
        }
    }
    result.push_str(rest);
    Ok(result)
}

pub fn resolve(root: &mut Node, global: &Constants) -> Result<()> {
    let Node::Table(table) = root else {
        unreachable!()
    };
    let local = table
        .remove("constants")
        .map(definitions)
        .transpose()?
        .unwrap_or_default();
    resolve_resources(root, &local, global, true)
}

fn resolve_resources(
    node: &mut Node,
    local: &Constants,
    global: &Constants,
    root: bool,
) -> Result<()> {
    match node {
        Node::String(value) => *value = expand(value, local, global)?,
        Node::Table(table) if table.contains_key("run") => {
            if let Some(Node::Array(args)) = table.get_mut("args") {
                for arg in args {
                    if let Node::Table(arg) = arg
                        && let Some(choices @ Node::Table(_)) = arg.get_mut("enum")
                    {
                        let Node::Table(reference) = choices else {
                            unreachable!()
                        };
                        ensure!(reference.len() == 1, "Enum reference accepts only ref");
                        let reference = reference
                            .get("ref")
                            .and_then(Node::as_str)
                            .context("Enum reference requires a string ref")?;
                        let Node::Array(values) = lookup(reference, local, global)? else {
                            bail!("Enum constant {reference} must be a list of strings");
                        };
                        *choices = Node::Array(
                            values
                                .iter()
                                .map(|v| Node::String(v.as_str().unwrap().to_owned()))
                                .collect(),
                        );
                    }
                }
            }
            if let Some(Node::Table(env)) = table.get_mut("env") {
                for value in env.values_mut() {
                    let text = value
                        .as_str()
                        .context("Environment values must be strings")?;
                    let expanded = expand(text, local, global)?;
                    let expanded = if expanded == "~" || expanded.starts_with("~/") {
                        crate::resource::normalize(&expanded)?
                    } else {
                        expanded
                    };
                    *value = Node::String(expanded);
                }
            }
        }
        Node::Table(table) => {
            let resource_object = table.contains_key("url") && table.contains_key("hint");
            for (name, value) in table {
                if matches!(name.as_str(), "browser" | "filehook" | "dirhook")
                    || (root && name == "alias")
                    || (resource_object && name == "hint")
                {
                    continue;
                }
                resolve_resources(value, local, global, false)?;
            }
        }
        Node::Array(_) => bail!("Arrays are only supported in constants and script args"),
    }
    Ok(())
}
