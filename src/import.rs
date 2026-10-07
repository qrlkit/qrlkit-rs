use crate::format::Node;
use crate::store::{CollisionStrategy, Entry, Source};
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

pub fn read(path: &Path, renames: BTreeMap<String, String>) -> Result<Source> {
    let path = fs::canonicalize(path).with_context(|| format!("Cannot find {}", path.display()))?;
    let text = fs::read_to_string(&path)?;
    let mut value = crate::format::parse(&path, &text)?;
    let hints = match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "toml" => crate::format::toml_hints(&text)?,
        "yaml" | "yml" => crate::format::yaml_hints(&text)?,
        _ => BTreeMap::new(),
    };
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
        None,
    )?;
    ensure!(!entries.is_empty(), "No resources in {}", path.display());
    for entry in &mut entries {
        if entry.script.is_none() {
            entry.url = crate::resource::normalize(&entry.url)?;
        }
        if entry.hint.is_none() {
            entry.hint = hints.get(&entry.key).cloned();
        }
        entry.key = map_key(&entry.key, &renames, false);
    }
    Ok(Source {
        collision_strategy: None,
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
    browser: Option<&str>,
) -> Result<()> {
    match value {
        Node::Table(table) => {
            let browser = match table.get("browser") {
                Some(value) => Some(value.as_str().context("browser must be a string")?),
                None => browser,
            };
            let filehook = match table.get("filehook") {
                Some(value) => {
                    let hook = value.as_str().context("filehook must be a string")?;
                    crate::filehook::validate(hook)?;
                    Some(hook)
                }
                None => filehook,
            };
            let dirhook = match table.get("dirhook") {
                Some(value) => {
                    let hook = value.as_str().context("dirhook must be a string")?;
                    crate::dirhook::validate(hook)?;
                    Some(hook)
                }
                None => dirhook,
            };
            if table.contains_key("url") && table.contains_key("hint") {
                ensure!(!key.is_empty(), "Resource objects must have a name");
                ensure!(
                    table.keys().all(|k| matches!(
                        k.as_str(),
                        "url" | "hint" | "browser" | "filehook" | "dirhook"
                    )),
                    "Resource {} cannot contain child resources or unknown settings",
                    key.join(" ")
                );
                let hint = table["hint"].as_str().context("hint must be a string")?;
                let url = table["url"].as_str().context("url must be a string")?;
                flatten(
                    &Node::String(url.to_owned()),
                    key,
                    entries,
                    source_dir,
                    filehook,
                    dirhook,
                    browser,
                )?;
                entries.last_mut().unwrap().hint =
                    Some(hint.trim().to_owned()).filter(|s| !s.is_empty());
                return Ok(());
            }
            if let Some(run) = table.get("run") {
                ensure!(!key.is_empty(), "run must belong to a named resource table");
                ensure!(
                    table
                        .keys()
                        .all(|k| matches!(k.as_str(), "run" | "shell" | "hint")),
                    "Script {} cannot contain child resources or unknown settings",
                    key.join(" ")
                );
                let body = run.as_str().context("run must be a string")?.to_owned();
                let shell = match table.get("shell") {
                    Some(value) => value.as_str().context("shell must be a string")?,
                    None => "bash",
                }
                .to_owned();
                let script = crate::script::Script {
                    body,
                    shell,
                    cwd: source_dir.to_path_buf(),
                };
                script.validate()?;
                entries.push(Entry {
                    hint: table
                        .get("hint")
                        .map(|value| value.as_str().context("hint must be a string"))
                        .transpose()?
                        .map(str::trim)
                        .filter(|hint| !hint.is_empty())
                        .map(str::to_owned),
                    key: key.clone(),
                    url: String::new(),
                    script: Some(script),
                    browser: None,
                    filehook: None,
                    dirhook: None,
                });
                return Ok(());
            }
            ensure!(
                !table.contains_key("shell"),
                "shell requires run at {}",
                key.join(" ")
            );
            for (segment, child) in table {
                if matches!(segment.as_str(), "filehook" | "dirhook" | "browser") {
                    continue;
                }
                ensure!(valid_segment(segment), "Invalid key segment: {segment:?}");
                key.push(segment.clone());
                flatten(child, key, entries, source_dir, filehook, dirhook, browser)?;
                key.pop();
            }
        }
        Node::String(url) => {
            crate::template::names(url)?;
            crate::resource::validate(url).with_context(|| format!("At {}", key.join(" ")))?;
            entries.push(Entry {
                hint: None,
                key: key.clone(),
                url: url.clone(),
                script: None,
                browser: browser.map(str::to_owned),
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

/// Shared namespaces merge; resources conflict with equal paths or descendants.
pub fn collision(
    sources: &[Source],
    strategy: CollisionStrategy,
) -> Option<(usize, usize, String)> {
    for (i, source) in sources.iter().enumerate() {
        for root in roots(source) {
            if reserved(&root) {
                return Some((i, i, root));
            }
        }
        for (j, other) in sources.iter().enumerate().take(i) {
            // Newer explicit choices take precedence; otherwise honor the older
            // file's override before falling back to the global default.
            let strategy = source
                .collision_strategy
                .or(other.collision_strategy)
                .unwrap_or(strategy);
            for entry in &source.entries {
                for other_entry in &other.entries {
                    let path = if strategy == CollisionStrategy::Rename {
                        (entry.key[0] == other_entry.key[0]).then_some(&entry.key[..1])
                    } else if entry.key.starts_with(&other_entry.key) {
                        Some(other_entry.key.as_slice())
                    } else if other_entry.key.starts_with(&entry.key) {
                        Some(entry.key.as_slice())
                    } else {
                        None
                    };
                    if let Some(path) = path {
                        return Some((j, i, path.join(" ")));
                    }
                }
            }
        }
    }
    None
}

/// Apply the most specific saved prefix once, using original paths as identity.
/// Segments cannot contain whitespace, so spaces encode paths without ambiguity.
pub fn map_key(key: &[String], renames: &BTreeMap<String, String>, reverse: bool) -> Vec<String> {
    for end in (1..=key.len()).rev() {
        let prefix = key[..end].join(" ");
        let replacement = if reverse {
            renames
                .iter()
                .find_map(|(original, alias)| (alias == &prefix).then_some(original))
        } else {
            renames.get(&prefix)
        };
        if let Some(replacement) = replacement {
            return replacement
                .split(' ')
                .map(str::to_owned)
                .chain(key[end..].iter().cloned())
                .collect();
        }
    }
    key.to_vec()
}

pub fn rename(source: &mut Source, path: &str, alias: &str) -> Result<()> {
    let key: Vec<String> = path.split(' ').map(str::to_owned).collect();
    ensure!(
        valid_segment(alias) && (key.len() > 1 || !reserved(alias)),
        "Enter one namespace without spaces or a reserved command name"
    );
    let mut target = key.clone();
    *target.last_mut().unwrap() = alias.into();
    ensure!(
        source
            .entries
            .iter()
            .any(|entry| entry.key.starts_with(&key)),
        "Unknown namespace: {path}"
    );
    ensure!(
        !source
            .entries
            .iter()
            .any(|entry| entry.key.starts_with(&target) || target.starts_with(&entry.key)),
        "Namespace {} already exists in this file",
        target.join(" ")
    );
    let original = map_key(&key, &source.renames, true).join(" ");
    // Keep descendant aliases attached when an ancestor is renamed later.
    for effective in source.renames.values_mut() {
        let parts: Vec<String> = effective.split(' ').map(str::to_owned).collect();
        if parts.starts_with(&key) {
            *effective = target
                .iter()
                .chain(&parts[key.len()..])
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
        }
    }
    source.renames.insert(original, target.join(" "));
    for entry in &mut source.entries {
        if entry.key.starts_with(&key) {
            entry.key[..key.len()].clone_from_slice(&target);
        }
    }
    Ok(())
}

pub fn reload(sources: &[Source]) -> Result<Vec<Source>> {
    sources.iter().map(|s| {
        // Validate and transform the same snapshot; do not reread a changing file.
        let mut loaded = read(&s.path, BTreeMap::new())?;
        let mut effective = BTreeMap::new();
        for entry in &mut loaded.entries {
            for end in 1..=entry.key.len() {
                let original = entry.key[..end].to_vec();
                let alias = map_key(&original, &s.renames, false);
                if let Some(previous) = effective.insert(alias.clone(), original.clone()) {
                    ensure!(previous == original, "Reload would merge namespaces into {} in {}; rename the new key in the config file first", alias.join(" "), s.path.display());
                }
            }
            entry.key = map_key(&entry.key, &s.renames, false);
        }
        loaded.renames = s.renames.clone();
        loaded.collision_strategy = s.collision_strategy;
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
    fn toml_hints_follow_resources_and_refresh_after_renaming() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = fixture(
            dir.path(),
            "hints.TOML",
            r#"
[web]
# hint: Opens repo
repo = 'https://example.com'
# hint: Literal dotted key
'api.v2' = 'https://example.com/v2'
plain = 'https://example.com/plain'
# hint: Detached

blank = 'https://example.com/blank'
# hint: Not the next resource
# ordinary comment
comment = 'https://example.com/comment'
# hint:
empty = 'https://example.com/empty'
# hint: Nested
nested.page = 'https://example.com/nested'
# hint: Runs a task
[task]
run = '''
# hint: Fake hint inside a script
web.repo = "not a resource"
'''
"#,
        );
        for (key, expected) in [
            ("web repo", Some("Opens repo")),
            ("web api.v2", Some("Literal dotted key")),
            ("web plain", None),
            ("web blank", None),
            ("web comment", None),
            ("web empty", None),
            ("web nested page", Some("Nested")),
            ("task", Some("Runs a task")),
        ] {
            let entry = source
                .entries
                .iter()
                .find(|e| e.key.join(" ") == key)
                .unwrap();
            assert_eq!(entry.hint.as_deref(), expected, "{key}");
        }
        rename(&mut source, "web", "links").unwrap();
        fs::write(
            &source.path,
            "[web]\n# hint: Updated\nrepo = 'https://example.com'",
        )
        .unwrap();
        let loaded = reload(&[source]).unwrap();
        assert_eq!(loaded[0].entries[0].key, ["links", "repo"]);
        assert_eq!(loaded[0].entries[0].hint.as_deref(), Some("Updated"));
        fs::write(&loaded[0].path, "[web]\nrepo = 'https://example.com'").unwrap();
        assert!(reload(&loaded).unwrap()[0].entries[0].hint.is_none());
    }

    #[test]
    fn file_overrides_take_precedence_over_defaults_and_newer_overrides_win() {
        let dir = tempfile::tempdir().unwrap();
        let mut first = fixture(dir.path(), "a.toml", "[qk]\na = 'https://a.test'");
        let mut second = fixture(dir.path(), "b.toml", "[qk]\nb = 'https://b.test'");
        for (older, newer, default, conflicts) in [
            (None, None, CollisionStrategy::Merge, false),
            (None, None, CollisionStrategy::Rename, true),
            (
                Some(CollisionStrategy::Merge),
                None,
                CollisionStrategy::Rename,
                false,
            ),
            (
                Some(CollisionStrategy::Rename),
                None,
                CollisionStrategy::Merge,
                true,
            ),
            (
                Some(CollisionStrategy::Rename),
                Some(CollisionStrategy::Merge),
                CollisionStrategy::Rename,
                false,
            ),
            (
                Some(CollisionStrategy::Merge),
                Some(CollisionStrategy::Rename),
                CollisionStrategy::Merge,
                true,
            ),
        ] {
            first.collision_strategy = older;
            second.collision_strategy = newer;
            let sources = reload(&[first.clone(), second.clone()]).unwrap();
            assert_eq!(sources[0].collision_strategy, older);
            assert_eq!(sources[1].collision_strategy, newer);
            assert_eq!(collision(&sources, default).is_some(), conflicts);
        }
    }

    #[test]
    fn merge_combines_namespaces_and_detects_exact_and_prefix_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let a = fixture(dir.path(), "a.toml", "[qk.git]\nprs = 'https://a.test'");
        let b = fixture(
            dir.path(),
            "b.yaml",
            "qk:\n  git:\n    issues: https://b.test",
        );
        assert!(collision(&[a.clone(), b.clone()], CollisionStrategy::Merge).is_none());
        assert_eq!(
            collision(&[a.clone(), b], CollisionStrategy::Rename),
            Some((0, 1, "qk".into()))
        );
        for text in [
            "[qk.git]\nprs = 'https://a.test'",
            "[qk.git.prs]\nchild = 'https://b.test'",
        ] {
            let b = fixture(dir.path(), "b.toml", text);
            for mut sources in [vec![a.clone(), b.clone()], vec![b, a.clone()]] {
                assert_eq!(
                    collision(&sources, CollisionStrategy::Merge),
                    Some((0, 1, "qk git prs".into()))
                );
                rename(&mut sources[1], "qk git prs", "team-prs").unwrap();
                assert!(collision(&sources, CollisionStrategy::Merge).is_none());
                let loaded = reload(&sources).unwrap();
                assert_eq!(loaded[1].entries[0].key, sources[1].entries[0].key);
            }
        }
    }

    #[test]
    fn nested_aliases_compose_with_ancestor_aliases_and_survive_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = fixture(
            dir.path(),
            "a.toml",
            "[qk.git]\n'api.v2' = 'https://a.test'\nissues = 'https://b.test'",
        );
        rename(&mut source, "qk git api.v2", "team-api").unwrap();
        rename(&mut source, "qk", "work").unwrap();
        rename(&mut source, "work git", "team-git").unwrap();
        rename(&mut source, "work team-git team-api", "nuke").unwrap();
        let loaded = reload(&[source]).unwrap();
        let key = vec!["work".into(), "team-git".into(), "nuke".into()];
        assert_eq!(loaded[0].entries[0].key, key);
        assert_eq!(
            map_key(&key, &loaded[0].renames, true),
            ["qk", "git", "api.v2"]
        );
        assert_eq!(loaded[0].entries[1].key, ["work", "team-git", "issues"]);
        fs::write(&loaded[0].path, "[qk.git]\nissues = 'https://b.test'").unwrap();
        let loaded = reload(&loaded).unwrap();
        fs::write(&loaded[0].path, "[qk.git]\n'api.v2' = 'https://new.test'").unwrap();
        assert_eq!(reload(&loaded).unwrap()[0].entries[0].key, key);
    }

    #[test]
    fn nested_aliases_reject_sibling_merges_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = fixture(
            dir.path(),
            "a.toml",
            "[qk.git]\nprs = 'https://a.test'\nissues = 'https://b.test'",
        );
        let before = serde_yaml_ng::to_string(&source).unwrap();
        assert!(rename(&mut source, "qk git prs", "issues").is_err());
        assert_eq!(serde_yaml_ng::to_string(&source).unwrap(), before);
        rename(&mut source, "qk git prs", "team-prs").unwrap();
        fs::write(
            &source.path,
            "[qk.git]\nprs = 'https://a.test'\n[ qk.git.team-prs ]\nchild = 'https://b.test'",
        )
        .unwrap();
        assert!(reload(&[source]).is_err());
    }

    #[test]
    fn script_roots_rename_and_reload_without_exposing_metadata_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut source = fixture(dir.path(), "script.toml", "[nuke]\nrun = 'echo first'\n");
        assert_eq!(source.entries[0].key, ["nuke"]);
        assert!(collision(std::slice::from_ref(&source), CollisionStrategy::Rename).is_some());
        rename(&mut source, "nuke", "cleanup").unwrap();
        fs::write(&source.path, "[nuke]\nrun = 'echo updated'\n").unwrap();
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
        assert!(collision(std::slice::from_ref(&source), CollisionStrategy::Rename).is_none());
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
            "set-collision-strategy",
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
            assert_eq!(
                collision(&[source.clone()], CollisionStrategy::Rename),
                Some((0, 0, root.into()))
            );
            assert!(rename(&mut source, root, "nuke").is_err());
            rename(&mut source, root, "team-links").unwrap();
            assert!(collision(std::slice::from_ref(&source), CollisionStrategy::Rename).is_none());
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
        assert_eq!(
            collision(&sources, CollisionStrategy::Rename),
            Some((0, 1, "git".into()))
        );
        rename(&mut sources[1], "git", "team2-git").unwrap();
        assert!(collision(&sources, CollisionStrategy::Rename).is_none());
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
        assert_eq!(
            collision(&[source], CollisionStrategy::Rename),
            Some((0, 0, "reload".into()))
        );
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;
    #[test]
    fn browser_settings_inherit_override_and_reset_across_formats() {
        let dir = tempfile::tempdir().unwrap();
        for (ext, text) in [
            (
                "toml",
                "browser = 'firefox'\na = 'https://example.com'\n[group]\nbrowser = 'other-browser'\nb = 'about:addons'\n[group.default]\nbrowser = ''\nc = 'https://example.com'",
            ),
            (
                "yaml",
                "browser: firefox\na: https://example.com\ngroup:\n  browser: other-browser\n  b: about:addons\n  default:\n    browser: ''\n    c: https://example.com",
            ),
            (
                "json",
                r#"{"browser":"firefox","a":"https://example.com","group":{"browser":"other-browser","b":"about:addons","default":{"browser":"","c":"https://example.com"}}}"#,
            ),
        ] {
            let path = dir.path().join(format!("browsers.{ext}"));
            fs::write(&path, text).unwrap();
            let source = read(&path, BTreeMap::new()).unwrap();
            assert_eq!(source.entries.len(), 3);
            for (key, expected) in [("a", "firefox"), ("b", "other-browser"), ("c", "")] {
                let entry = source
                    .entries
                    .iter()
                    .find(|e| e.key.last().unwrap() == key)
                    .unwrap();
                assert_eq!(entry.browser.as_deref(), Some(expected));
            }
        }
        let path = dir.path().join("invalid.toml");
        fs::write(&path, "[browser]\nx = 'https://example.com'").unwrap();
        assert!(
            read(&path, BTreeMap::new())
                .unwrap_err()
                .to_string()
                .contains("browser must be a string")
        );
    }

    #[test]
    fn filehook_metadata_is_inherited_across_formats_and_validated() {
        let dir = tempfile::tempdir().unwrap();
        for (ext, text) in [
            (
                "toml",
                "filehook = 'nvim $file'\n[notes]\nx = './{name}'\n[notes.raw]\nfilehook = ''\nx = './raw'",
            ),
            (
                "yaml",
                "filehook: nvim $file\nnotes:\n  x: './{name}'\n  raw:\n    filehook: ''\n    x: ./raw",
            ),
            (
                "json",
                r#"{"filehook":"nvim $file","notes":{"x":"./{name}","raw":{"filehook":"","x":"./raw"}}}"#,
            ),
        ] {
            let path = dir.path().join(format!("hooks.{ext}"));
            fs::write(&path, text).unwrap();
            let source = read(&path, BTreeMap::new()).unwrap();
            assert_eq!(source.entries.len(), 2);
            let templated = source.entries.iter().find(|e| e.url == "./{name}").unwrap();
            assert_eq!(templated.filehook.as_deref(), Some("nvim $file"));
            let raw = source.entries.iter().find(|e| e.url == "./raw").unwrap();
            assert_eq!(raw.filehook.as_deref(), Some(""));
        }
        let path = dir.path().join("invalid.toml");
        for text in [
            "filehook = 'nvim'\nx = './file'",
            "[filehook]\nx = './file'",
            "[script]\nrun = 'echo hi'\nfilehook = 'nvim file'",
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
                "dirhook = 'cd $dir && pwd'\n[work]\nrepo = './repo'\n[work.quiet]\ndirhook = ''\nrepo = './repo'",
            ),
            (
                "yaml",
                "dirhook: cd $dir && pwd\nwork:\n  repo: ./repo\n  quiet:\n    dirhook: ''\n    repo: ./repo",
            ),
            (
                "json",
                r#"{"dirhook":"cd $dir && pwd","work":{"repo":"./repo","quiet":{"dirhook":"","repo":"./repo"}}}"#,
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
                        "cd $dir && pwd"
                    } else {
                        ""
                    })
                );
            }
        }
        let path = dir.path().join("bad.toml");
        for text in [
            "dirhook = 'cd'\nx = './repo'",
            "[dirhook]\nx = './repo'",
            "[script]\nrun = 'echo hi'\ndirhook = 'cd dir'",
        ] {
            fs::write(&path, text).unwrap();
            assert!(read(&path, BTreeMap::new()).is_err());
        }
    }

    #[test]
    fn yaml_hints_use_key_locations_and_ignore_scalar_contents() {
        let dir = tempfile::tempdir().unwrap();
        let source = {
            let path = dir.path().join("hints.yml");
            fs::write(
                &path,
                r#"
web:
  # hint: Opens ø repo
  'repo:main': https://example.com
  # hint: Detached

  plain: https://example.com/plain
  # hint: Empty
  # ordinary comment
  other: https://example.com/other
  nested:
    # hint: Nested page
    page: https://example.com/page
# hint: Runs task
task:
  run: |
    # hint: Not metadata
    web: fake
quoted:
  run: "echo hello
    # hint: Not a real key
    web: fake"
"#,
            )
            .unwrap();
            read(&path, BTreeMap::new()).unwrap()
        };
        for (key, hint) in [
            ("web repo:main", Some("Opens ø repo")),
            ("web plain", None),
            ("web other", None),
            ("web nested page", Some("Nested page")),
            ("task", Some("Runs task")),
            ("quoted", None),
        ] {
            let entry = source
                .entries
                .iter()
                .find(|e| e.key.join(" ") == key)
                .unwrap();
            assert_eq!(entry.hint.as_deref(), hint, "{key}");
        }
        let windows = fs::read_to_string(&source.path)
            .unwrap()
            .replace('\n', "\r\n");
        fs::write(&source.path, windows).unwrap();
        assert_eq!(
            serde_json::to_value(&source.entries).unwrap(),
            serde_json::to_value(&reload(&[source]).unwrap()[0].entries).unwrap()
        );
    }

    #[test]
    fn resource_objects_validate_hints_and_inherit_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hints.json");
        fs::write(&path, r#"{"browser":"firefox","web":{"repo":{"url":"https://example.com","hint":" Opens repo "}},"task":{"run":"echo hi","hint":"Runs task"}}"#).unwrap();
        let source = read(&path, BTreeMap::new()).unwrap();
        let repo = source
            .entries
            .iter()
            .find(|e| e.key == ["web", "repo"])
            .unwrap();
        assert_eq!(repo.hint.as_deref(), Some("Opens repo"));
        assert_eq!(repo.browser.as_deref(), Some("firefox"));
        assert_eq!(
            source
                .entries
                .iter()
                .find(|e| e.key == ["task"])
                .unwrap()
                .hint
                .as_deref(),
            Some("Runs task")
        );
        for text in [
            r#"{"repo":{"url":"https://example.com","hint":42}}"#,
            r#"{"repo":{"url":"https://example.com","hint":{}}}"#,
            r#"{"repo":{"url":{},"hint":"Opens repo"}}"#,
            r#"{"repo":{"url":"https://example.com","hint":"Opens repo","child":"./file"}}"#,
            r#"{"repo":{"url":"https://example.com","hint":"Opens repo","run":"echo hi"}}"#,
            r#"{"task":{"run":"echo hi","hint":{}}}"#,
        ] {
            fs::write(&path, text).unwrap();
            assert!(read(&path, BTreeMap::new()).is_err(), "{text}");
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
            assert_eq!(source.entries.len(), 5);
            for (key, hint) in [
                ("web docs", "Opens the documentation"),
                ("paths repo", "Opens repo"),
            ] {
                let entry = source
                    .entries
                    .iter()
                    .find(|entry| entry.key.join(" ") == key)
                    .unwrap();
                assert_eq!(entry.hint.as_deref(), Some(hint), "{extension}: {key}");
            }
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
        assert_eq!(
            collision(&sources, CollisionStrategy::Rename),
            Some((0, 1, "web".into()))
        );
        rename(&mut sources[1], "web", "yaml-web").unwrap();
        rename(&mut sources[2], "web", "json-web").unwrap();
        fs::write(&sources[1].path, "web:\n  new: https://new.test").unwrap();
        let loaded = reload(&sources).unwrap();
        assert!(collision(&loaded, CollisionStrategy::Rename).is_none());
        assert_eq!(loaded[1].entries[0].key, ["yaml-web", "new"]);
        fs::write(&sources[2].path, r#"{"web":{"shell":"bash"}}"#).unwrap();
        assert!(reload(&sources).is_err());
    }
}
