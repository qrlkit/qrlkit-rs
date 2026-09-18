use crate::format::Node;
use crate::store::{Entry, Source};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// Expand a directory deterministically, without importing nested directories.
pub fn paths(path: &Path) -> Result<Vec<PathBuf>> {
    if !fs::metadata(path)
        .with_context(|| format!("Cannot access {}", path.display()))?
        .is_dir()
    {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut paths = Vec::new();
    for entry in
        fs::read_dir(path).with_context(|| format!("Cannot read directory {}", path.display()))?
    {
        let entry = entry?;
        let candidate = entry.path();
        let extension = candidate.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(
            extension.to_ascii_lowercase().as_str(),
            "toml" | "yaml" | "yml" | "json"
        ) && fs::metadata(&candidate)
            .with_context(|| format!("Cannot access {}", candidate.display()))?
            .is_file()
        {
            paths.push(candidate);
        }
    }
    paths.sort();
    ensure!(
        !paths.is_empty(),
        "No supported config files in {}",
        path.display()
    );
    Ok(paths)
}

fn reserved(key: &str) -> bool {
    use clap::CommandFactory;
    let mut command = crate::Cli::command();
    command.build();
    command.get_subcommands().any(|subcommand| {
        subcommand.get_name() == key || subcommand.get_all_aliases().any(|alias| alias == key)
    })
}

pub fn valid_segment(key: &str) -> bool {
    !key.is_empty()
        && !key.starts_with('-')
        && !key.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub fn validate_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value).context("Invalid URL")?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "Expected an absolute HTTP(S) URL: {value}"
    );
    Ok(())
}

pub fn read(path: &Path, renames: BTreeMap<String, String>) -> Result<Source> {
    let path = fs::canonicalize(path).with_context(|| format!("Cannot find {}", path.display()))?;
    let mut value = crate::format::parse(&path, &fs::read_to_string(&path)?)?;
    let alias = if let Node::Table(table) = &mut value {
        match table.remove("alias") {
            Some(Node::Table(mut metadata)) => {
                let name = metadata.remove("name").context("alias requires name")?;
                ensure!(metadata.is_empty(), "alias accepts only name");
                let name = name
                    .as_str()
                    .context("alias.name must be a string")?
                    .to_owned();
                crate::alias::valid_name(&name)?;
                Some(name)
            }
            Some(_) => anyhow::bail!("alias must be an object containing name"),
            None => None,
        }
    } else {
        None
    };
    let mut entries = vec![];
    flatten(
        &value,
        &mut vec![],
        &mut entries,
        path.parent().unwrap(),
        None,
        None,
    )?;
    ensure!(!entries.is_empty(), "No resources in {}", path.display());
    for entry in &mut entries {
        if entry.script.is_none() {
            entry.url = crate::resource::normalize(&entry.url)?;
        }
        if let Some(alias) = renames.get(&entry.key[0]) {
            entry.key[0] = alias.clone();
        }
    }
    Ok(Source {
        alias,
        path,
        renames,
        entries,
    })
}

fn flatten(
    value: &Node,
    key: &mut Vec<String>,
    entries: &mut Vec<Entry>,
    source_dir: &Path,
    filehook: Option<&str>,
    dirhook: Option<&str>,
) -> Result<()> {
    match value {
        Node::Table(table) => {
            let filehook = match table.get("$filehook") {
                Some(value) => {
                    let hook = value.as_str().context("$filehook must be a string")?;
                    crate::filehook::validate(hook)?;
                    Some(hook)
                }
                None => filehook,
            };
            let dirhook = match table.get("$dirhook") {
                Some(value) => {
                    let hook = value.as_str().context("$dirhook must be a string")?;
                    crate::dirhook::validate(hook)?;
                    Some(hook)
                }
                None => dirhook,
            };
            if let Some(run) = table.get("$run") {
                ensure!(
                    !key.is_empty(),
                    "$run must belong to a named resource table"
                );
                ensure!(
                    table
                        .keys()
                        .all(|k| matches!(k.as_str(), "$run" | "$shell")),
                    "Script {} cannot contain child resources or unknown settings",
                    key.join(" ")
                );
                let body = run.as_str().context("$run must be a string")?.to_owned();
                let shell = match table.get("$shell") {
                    Some(value) => value.as_str().context("$shell must be a string")?,
                    None => {
                        if cfg!(windows) {
                            "pwsh"
                        } else {
                            "bash"
                        }
                    }
                }
                .to_owned();
                let script = crate::script::Script {
                    body,
                    shell,
                    cwd: source_dir.to_path_buf(),
                };
                script.validate()?;
                entries.push(Entry {
                    key: key.clone(),
                    url: String::new(),
                    script: Some(script),
                    filehook: None,
                    dirhook: None,
                });
                return Ok(());
            }
            ensure!(
                !table.contains_key("$shell"),
                "$shell requires $run at {}",
                key.join(" ")
            );
            for (segment, child) in table {
                if matches!(segment.as_str(), "$filehook" | "$dirhook") {
                    continue;
                }
                ensure!(valid_segment(segment), "Invalid key segment: {segment:?}");
                key.push(segment.clone());
                flatten(child, key, entries, source_dir, filehook, dirhook)?;
                key.pop();
            }
        }
        Node::String(url) => {
            crate::template::names(url)?;
            crate::resource::validate(url).with_context(|| format!("At {}", key.join(" ")))?;
            entries.push(Entry {
                key: key.clone(),
                url: url.clone(),
                script: None,
                filehook: filehook.map(str::to_owned),
                dirhook: dirhook.map(str::to_owned),
            });
        }
    }
    Ok(())
}

pub fn roots(source: &Source) -> BTreeSet<String> {
    source.entries.iter().map(|e| e.key[0].clone()).collect()
}

/// Returns the highest conflicting namespace, including collisions inside one source.
pub fn collision(sources: &[Source]) -> Option<(usize, usize, String)> {
    for (i, source) in sources.iter().enumerate() {
        for root in roots(source) {
            if reserved(&root) {
                return Some((i, i, root));
            }
            for (j, other) in sources.iter().enumerate().take(i) {
                if roots(other).contains(&root) {
                    return Some((j, i, root));
                }
            }
        }
    }
    None
}

pub fn rename(source: &mut Source, root: &str, alias: &str) -> Result<()> {
    ensure!(
        valid_segment(alias) && !reserved(alias),
        "Enter one namespace without spaces or a reserved command name"
    );
    ensure!(
        !roots(source).contains(alias),
        "Namespace {alias} already exists in this file"
    );
    let originals: Vec<_> = source
        .renames
        .iter()
        .filter(|(_, v)| v.as_str() == root)
        .map(|(k, _)| k.clone())
        .collect();
    if originals.is_empty() {
        source.renames.insert(root.into(), alias.into());
    } else {
        for original in originals {
            source.renames.insert(original, alias.into());
        }
    }
    for entry in &mut source.entries {
        if entry.key[0] == root {
            entry.key[0] = alias.into();
        }
    }
    Ok(())
}

pub fn reload(sources: &[Source]) -> Result<Vec<Source>> {
    sources.iter().map(|s| {
        // Validate and transform the same snapshot; do not reread a changing file.
        let mut loaded = read(&s.path, BTreeMap::new())?;
        let mut effective = BTreeSet::new();
        for root in roots(&loaded) {
            let alias = s.renames.get(&root).cloned().unwrap_or(root);
            ensure!(effective.insert(alias.clone()), "Reload would merge namespaces into {alias} in {}; rename the new root in the config file first", s.path.display());
        }
        for entry in &mut loaded.entries {
            if let Some(alias) = s.renames.get(&entry.key[0]) {
                entry.key[0] = alias.clone();
            }
        }
        loaded.renames = s.renames.clone();
        Ok(loaded)
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(dir: &Path, name: &str, text: &str) -> Source {
        let path = dir.join(name);
        fs::write(&path, text).unwrap();
        read(&path, BTreeMap::new()).unwrap()
    }
    #[test]
    fn script_roots_rename_and_reload_without_exposing_metadata_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = fixture(
            dir.path(),
            "script.toml",
            "[nuke]\n\"$run\" = 'echo first'\n",
        );
        assert_eq!(source.entries[0].key, ["nuke"]);
        assert!(collision(std::slice::from_ref(&source)).is_some());
        rename(&mut source, "nuke", "cleanup").unwrap();
        fs::write(&source.path, "[nuke]\n\"$run\" = 'echo updated'\n").unwrap();
        let loaded = reload(&[source]).unwrap();
        assert_eq!(loaded[0].entries[0].key, ["cleanup"]);
        let script = loaded[0].entries[0].script.as_ref().unwrap();
        assert_eq!(script.body, "echo updated");
        assert_eq!(script.cwd, fs::canonicalize(dir.path()).unwrap());
    }

    #[test]
    fn quoted_dots_unicode_root_urls_and_nested_commands_remain_literal() {
        let dir = tempfile::tempdir().unwrap();
        let source = fixture(
            dir.path(),
            "literal.toml",
            "home = 'https://example.com'\n[git]\n'api.v2' = 'https://example.com/v2'\n[git.nuke]\n'øvelse' = 'https://example.com/unicode'",
        );
        assert!(collision(std::slice::from_ref(&source)).is_none());
        for key in [
            vec!["home"],
            vec!["git", "api.v2"],
            vec!["git", "nuke", "øvelse"],
        ] {
            assert!(source.entries.iter().any(|entry| entry.key == key));
        }
    }

    #[test]
    fn rejected_renames_leave_aliases_and_entries_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = fixture(
            dir.path(),
            "a.toml",
            "[git]\nprs = 'https://a.test'\n[logs]\napi = 'https://b.test'",
        );
        let before = serde_yaml_ng::to_string(&source).unwrap();
        for alias in ["", "two words", "-flag", "nuke", "git", "logs", "bad\nkey"] {
            assert!(rename(&mut source, "git", alias).is_err());
            assert_eq!(serde_yaml_ng::to_string(&source).unwrap(), before);
        }
    }

    #[test]
    fn reload_drops_deleted_urls_but_keeps_alias_for_returning_root() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = fixture(
            dir.path(),
            "a.toml",
            "[git]\nprs = 'https://a.test'\n[logs]\napi = 'https://b.test'",
        );
        rename(&mut source, "git", "work-git").unwrap();
        fs::write(&source.path, "[logs]\napi = 'https://b.test'").unwrap();
        let loaded = reload(&[source]).unwrap();
        assert_eq!(roots(&loaded[0]), BTreeSet::from(["logs".into()]));
        fs::write(&loaded[0].path, "[git]\nnew = 'https://new.test'").unwrap();
        let loaded = reload(&loaded).unwrap();
        assert_eq!(loaded[0].entries[0].key, ["work-git", "new"]);
        assert_eq!(loaded[0].entries.len(), 1);
    }

    #[test]
    fn all_builtin_roots_require_renaming_and_cannot_be_aliases() {
        let dir = tempfile::tempdir().unwrap();
        for root in [
            "add",
            "ls",
            "rm",
            "reload",
            "set-browser",
            "set-filehook",
            "set-dirhook",
            "nuke",
            "init",
            "help",
        ] {
            let mut source = fixture(
                dir.path(),
                "commands.toml",
                &format!("[{root}]\nrepo = 'https://example.com'"),
            );
            assert_eq!(collision(&[source.clone()]), Some((0, 0, root.into())));
            assert!(rename(&mut source, root, "nuke").is_err());
            rename(&mut source, root, "team-links").unwrap();
            assert!(collision(std::slice::from_ref(&source)).is_none());
            assert_eq!(source.entries[0].key, ["team-links", "repo"]);
        }
        assert!(!reserved("prs"));
    }

    #[test]
    fn shared_roots_collide_and_rename_entire_subtree() {
        let dir = tempfile::tempdir().unwrap();
        let a = fixture(dir.path(), "a.toml", "[git]\nprs = 'https://a.test'");
        let b = fixture(dir.path(), "b.toml", "[git]\nissues = 'https://b.test'");
        let mut sources = vec![a, b];
        assert_eq!(collision(&sources), Some((0, 1, "git".into())));
        rename(&mut sources[1], "git", "team2-git").unwrap();
        assert!(collision(&sources).is_none());
        assert_eq!(sources[1].entries[0].key, ["team2-git", "issues"]);
    }
    #[test]
    fn reload_keeps_chained_aliases_and_new_descendants() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = fixture(dir.path(), "a.toml", "[git]\nprs = 'https://old.test'");
        rename(&mut source, "git", "team-git").unwrap();
        rename(&mut source, "team-git", "work-git").unwrap();
        fs::write(
            &source.path,
            "[git]\nprs = 'https://new.test'\nissues = 'https://issues.test'",
        )
        .unwrap();
        let loaded = reload(&[source]).unwrap();
        assert_eq!(roots(&loaded[0]), BTreeSet::from(["work-git".into()]));
        assert_eq!(loaded[0].entries.len(), 2);
        assert!(
            loaded[0]
                .entries
                .iter()
                .any(|e| e.url == "https://new.test")
        );
    }
    #[test]
    fn reload_rejects_internal_alias_merges_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = fixture(dir.path(), "a.toml", "[git]\nprs = 'https://a.test'");
        rename(&mut source, "git", "team").unwrap();
        fs::write(
            &source.path,
            "[git]\nprs = 'https://a.test'\n[team]\nissues = 'https://b.test'",
        )
        .unwrap();
        assert!(reload(&[source.clone()]).is_err());
        fs::remove_file(&source.path).unwrap();
        assert!(reload(&[source]).is_err());
    }
    #[test]
    fn invalid_leaves_and_reserved_names() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        for text in [
            "",
            "[git]\nprs = true",
            "[git]\nprs = [1, 2]",
            "[git]\nprs = 'ambiguous/path'",
            "[git]\nprs = 'file:///tmp/test'",
            "[git]\nprs = 'https://a.test'\nprs = 'https://b.test'",
            "[git]\nprs = 42",
            "[git]\nprs = 'javascript:alert(1)'",
            "['bad key']\na = 'https://a.test'",
            "[empty]",
        ] {
            fs::write(&path, text).unwrap();
            assert!(read(&path, BTreeMap::new()).is_err(), "{text}");
        }
        let source = fixture(
            dir.path(),
            "reserved.toml",
            "[reload]\na = 'https://a.test'",
        );
        assert_eq!(collision(&[source]), Some((0, 0, "reload".into())));
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;
    #[test]
    fn filehook_metadata_is_inherited_across_formats_and_validated() {
        let dir = tempfile::tempdir().unwrap();
        for (ext, text) in [
            (
                "toml",
                "\"$filehook\" = 'nvim file'\n[notes]\nx = './{name}'\n[notes.raw]\n\"$filehook\" = ''\nx = './raw'",
            ),
            (
                "yaml",
                "'$filehook': nvim file\nnotes:\n  x: './{name}'\n  raw:\n    '$filehook': ''\n    x: ./raw",
            ),
            (
                "json",
                r#"{"$filehook":"nvim file","notes":{"x":"./{name}","raw":{"$filehook":"","x":"./raw"}}}"#,
            ),
        ] {
            let path = dir.path().join(format!("hooks.{ext}"));
            fs::write(&path, text).unwrap();
            let source = read(&path, BTreeMap::new()).unwrap();
            assert_eq!(source.entries.len(), 2);
            let templated = source.entries.iter().find(|e| e.url == "./{name}").unwrap();
            assert_eq!(templated.filehook.as_deref(), Some("nvim file"));
            let raw = source.entries.iter().find(|e| e.url == "./raw").unwrap();
            assert_eq!(raw.filehook.as_deref(), Some(""));
        }
        let path = dir.path().join("invalid.toml");
        for text in [
            "\"$filehook\" = 'nvim'\nx = './file'",
            "[\"$filehook\"]\nx = './file'",
            "[script]\n\"$run\" = 'echo hi'\n\"$filehook\" = 'nvim file'",
        ] {
            fs::write(&path, text).unwrap();
            assert!(read(&path, BTreeMap::new()).is_err());
        }
    }

    #[test]
    fn directory_hook_settings_work_across_formats() {
        let dir = tempfile::tempdir().unwrap();
        for (extension, text) in [
            (
                "toml",
                "\"$dirhook\" = 'cd dir && pwd'\n[work]\nrepo = './repo'\n[work.quiet]\n\"$dirhook\" = ''\nrepo = './repo'",
            ),
            (
                "yaml",
                "'$dirhook': cd dir && pwd\nwork:\n  repo: ./repo\n  quiet:\n    '$dirhook': ''\n    repo: ./repo",
            ),
            (
                "json",
                r#"{"$dirhook":"cd dir && pwd","work":{"repo":"./repo","quiet":{"$dirhook":"","repo":"./repo"}}}"#,
            ),
        ] {
            let path = dir.path().join(format!("hooks.{extension}"));
            fs::write(&path, text).unwrap();
            let source = read(&path, BTreeMap::new()).unwrap();
            assert_eq!(source.entries.len(), 2);
            for entry in source.entries {
                assert_eq!(
                    entry.dirhook.as_deref(),
                    Some(if entry.key.len() == 2 {
                        "cd dir && pwd"
                    } else {
                        ""
                    })
                );
            }
        }
        let path = dir.path().join("bad.toml");
        for text in [
            "\"$dirhook\" = 'cd'\nx = './repo'",
            "[\"$dirhook\"]\nx = './repo'",
            "[script]\n\"$run\" = 'echo hi'\n\"$dirhook\" = 'cd dir'",
        ] {
            fs::write(&path, text).unwrap();
            assert!(read(&path, BTreeMap::new()).is_err());
        }
    }

    #[test]
    fn shipped_examples_have_identical_resources() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
        let mut snapshots = vec![];
        for extension in ["toml", "yaml", "json"] {
            let source = read(
                &examples.join(format!("syntax.{extension}")),
                BTreeMap::new(),
            )
            .unwrap();
            assert_eq!(source.entries.len(), 6);
            snapshots.push(serde_yaml_ng::to_string(&source.entries).unwrap());
        }
        assert!(snapshots.windows(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn mixed_formats_share_collisions_and_preserve_renames_on_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut sources = vec![];
        for (ext, text) in [
            ("toml", "[web]\nx = 'https://a.test'"),
            ("yaml", "web:\n  x: https://b.test"),
            ("json", r#"{"web":{"x":"https://c.test"}}"#),
        ] {
            let path = dir.path().join(format!("source.{ext}"));
            fs::write(&path, text).unwrap();
            sources.push(read(&path, BTreeMap::new()).unwrap());
        }
        assert_eq!(collision(&sources), Some((0, 1, "web".into())));
        rename(&mut sources[1], "web", "yaml-web").unwrap();
        rename(&mut sources[2], "web", "json-web").unwrap();
        fs::write(&sources[1].path, "web:\n  new: https://new.test").unwrap();
        let loaded = reload(&sources).unwrap();
        assert!(collision(&loaded).is_none());
        assert_eq!(loaded[1].entries[0].key, ["yaml-web", "new"]);
        fs::write(&sources[2].path, r#"{"web":{"$shell":"bash"}}"#).unwrap();
        assert!(reload(&sources).is_err());
    }
}
