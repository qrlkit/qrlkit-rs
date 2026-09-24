use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Browser {
    pub name: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entry {
    pub key: Vec<String>,
    // Keep the original YAML field name for compatibility with URL-only state.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<crate::script::Script>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filehook: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirhook: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Source {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub path: PathBuf,
    /// Original root -> effective root. Retained across reloads.
    #[serde(default)]
    pub renames: BTreeMap<String, String>,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State {
    pub version: u32,
    pub browser: Option<Browser>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<crate::shell::Shell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filehook: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirhook: Option<String>,
    pub sources: Vec<Source>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: 1,
            browser: None,
            shell: None,
            filehook: None,
            dirhook: None,
            sources: vec![],
        }
    }
}

impl State {
    pub fn load(path: &Path) -> Result<Self> {
        let content = match fs::read_to_string(path) {
            Ok(content) => content,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| format!("Cannot read {}", path.display())),
        };
        let state: Self = serde_yaml_ng::from_str(&content).context("Invalid QRL state YAML")?;
        ensure!(
            state.version == 1,
            "Unsupported state version {}",
            state.version
        );
        state
            .validate()
            .context("Invalid QRL state; fix the YAML or run qrlkit nuke")?;
        Ok(state)
    }

    fn validate(&self) -> Result<()> {
        if let Some(hook) = &self.filehook {
            crate::filehook::validate(hook)?;
        }
        if let Some(hook) = &self.dirhook {
            crate::dirhook::validate(hook)?;
        }
        crate::alias::validate(&self.sources)?;
        let mut keys = std::collections::BTreeSet::new();
        for source in &self.sources {
            for entry in &source.entries {
                ensure!(
                    !entry.key.is_empty()
                        && entry.key.iter().all(|k| crate::import::valid_segment(k)),
                    "Invalid key in {}: {:?}",
                    source.path.display(),
                    entry.key
                );
                if let Some(hook) = &entry.filehook {
                    crate::filehook::validate(hook)?;
                }
                if let Some(hook) = &entry.dirhook {
                    crate::dirhook::validate(hook)?;
                }
                if let Some(script) = &entry.script {
                    ensure!(
                        entry.url.is_empty(),
                        "Resource cannot be both a script and a URL/path"
                    );
                    script.validate()?;
                } else {
                    crate::resource::validate(&entry.url)?;
                }
                ensure!(
                    keys.insert(entry.key.clone()),
                    "Duplicate key: {}",
                    entry.key.join(" ")
                );
            }
            for (original, alias) in &source.renames {
                ensure!(
                    crate::import::valid_segment(original) && crate::import::valid_segment(alias),
                    "Invalid namespace rename in {}",
                    source.path.display()
                );
            }
        }
        for key in &keys {
            for end in 1..key.len() {
                ensure!(
                    !keys.contains(&key[..end]),
                    "Key is both a URL and a namespace: {}",
                    key[..end].join(" ")
                );
            }
        }
        Ok(())
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(serde_yaml_ng::to_string(self)?.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist(path)
            .with_context(|| format!("Cannot save {}", path.display()))?;
        Ok(())
    }

    /// Sorted, distinct immediate children of a path (not a fuzzy key search).
    pub fn children(&self, prefix: &[String]) -> Vec<String> {
        self.sources
            .iter()
            .flat_map(|source| &source.entries)
            .filter(|entry| entry.key.starts_with(prefix))
            .filter_map(|entry| entry.key.get(prefix.len()).cloned())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn lookup(&self, key: &[String]) -> Result<&Entry> {
        let matches: Vec<_> = self
            .sources
            .iter()
            .flat_map(|s| &s.entries)
            .filter(|e| e.key == key)
            .collect();
        ensure!(
            matches.len() <= 1,
            "Ambiguous key in state: {}",
            key.join(" ")
        );
        matches
            .first()
            .copied()
            .with_context(|| format!("No resource for: qrlkit {}", key.join(" ")))
    }
}

pub fn default_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot locate home directory; use --config <path>")?;
    let root = state_root(&home);
    Ok(root.join("qrlkit/state.yaml"))
}

fn state_root(home: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support")
    }

    #[cfg(target_os = "linux")]
    {
        std::env::var_os("XDG_STATE_HOME")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        dirs::data_local_dir().unwrap_or_else(|| home.join(".local/state"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_save_preserves_existing_directory_and_removes_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("state.yaml");
        fs::create_dir(&destination).unwrap();
        let marker = destination.join("keep.txt");
        fs::write(&marker, "untouched").unwrap();
        assert!(State::default().save(&destination).is_err());
        assert_eq!(fs::read_to_string(marker).unwrap(), "untouched");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn children_are_immediate_sorted_unique_and_prefix_scoped() {
        let mut state = State::default();
        state.sources.push(Source {
            alias: None,
            path: "test.toml".into(),
            renames: BTreeMap::new(),
            entries: [
                vec!["gh", "prs", "b"],
                vec!["gh", "prs", "a"],
                vec!["gh", "repo", "a"],
                vec!["logs", "api"],
            ]
            .into_iter()
            .map(|key| Entry {
                key: key.into_iter().map(String::from).collect(),
                url: "https://example.com".into(),
                script: None,
                filehook: None,
                dirhook: None,
            })
            .collect(),
        });
        assert_eq!(state.children(&[]), ["gh", "logs"]);
        assert_eq!(state.children(&["gh".into()]), ["prs", "repo"]);
        assert_eq!(state.children(&["gh".into(), "prs".into()]), ["a", "b"]);
        assert!(state.children(&["prs".into()]).is_empty());
        assert!(
            state
                .children(&["gh".into(), "prs".into(), "a".into()])
                .is_empty()
        );
    }

    #[test]
    fn state_roundtrip_replaces_and_resolves_exact_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/state.yaml");
        let mut state = State::default();
        state.sources.push(Source {
            alias: None,
            path: "team.toml".into(),
            renames: BTreeMap::new(),
            entries: vec![Entry {
                key: vec!["git".into(), "prs".into()],
                url: "https://example.com".into(),
                script: None,
                filehook: None,
                dirhook: None,
            }],
        });
        state.save(&path).unwrap();
        let loaded = State::load(&path).unwrap();
        assert_eq!(
            loaded.lookup(&["git".into(), "prs".into()]).unwrap().url,
            "https://example.com"
        );
        assert!(loaded.lookup(&["git".into()]).is_err());
        state.sources.clear();
        state.save(&path).unwrap();
        assert!(State::load(&path).unwrap().sources.is_empty());
    }
    #[test]
    fn corrupt_or_future_state_is_not_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.yaml");
        assert!(State::load(&path).unwrap().sources.is_empty());
        fs::write(&path, "{broken").unwrap();
        assert!(State::load(&path).is_err());
        fs::write(&path, "version: 2\nbrowser: null\nsources: []").unwrap();
        assert!(State::load(&path).is_err());
    }
}
