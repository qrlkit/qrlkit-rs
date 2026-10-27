use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Entry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<crate::script::Choice>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<String>,
    pub key: Vec<String>,
    // Keep the original YAML field name for compatibility with URL-only state.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<crate::script::Script>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<crate::script::Argument>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filehook: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirhook: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Group {
    pub key: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(default)]
    pub hidden: bool,
}

impl Entry {
    pub fn is_data(&self) -> bool {
        self.value.is_some() || self.choices.is_some()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Source {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<Group>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub themes: BTreeMap<String, crate::theme::Theme>,
    /// Explicit per-file override; absent means use the global default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collision_strategy: Option<CollisionStrategy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    pub path: PathBuf,
    /// Original path -> effective path, separated by spaces. Retained across reloads.
    #[serde(default)]
    pub renames: BTreeMap<String, String>,
    /// Removed prefixes in original source keys, retained across reloads and renames.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed_keys: Vec<Vec<String>>,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum CollisionStrategy {
    #[default]
    Merge,
    Rename,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct State {
    pub version: u32,
    #[serde(default = "default_theme_name")]
    pub default_theme: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom_themes: BTreeMap<String, crate::theme::Theme>,
    #[serde(default = "tool_themes_enabled")]
    pub tool_themes: bool,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub denied_theme_tools: BTreeSet<String>,
    #[serde(default)]
    pub collision_strategy: CollisionStrategy,
    pub browser: Option<Browser>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<crate::shell::Shell>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filehook: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirhook: Option<String>,
    pub sources: Vec<Source>,
}

fn default_theme_name() -> String {
    "terminal".into()
}
fn tool_themes_enabled() -> bool {
    true
}

impl Default for State {
    fn default() -> Self {
        Self {
            version: 2,
            default_theme: default_theme_name(),
            custom_themes: BTreeMap::new(),
            tool_themes: true,
            denied_theme_tools: BTreeSet::new(),
            collision_strategy: CollisionStrategy::default(),
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
            matches!(state.version, 1 | 2),
            "Unsupported state version {}",
            state.version
        );
        state
            .validate()
            .context("Invalid QRL state; fix the YAML or run qrlkit nuke")?;
        Ok(state)
    }

    fn validate(&self) -> Result<()> {
        for (name, theme) in &self.custom_themes {
            crate::theme::valid_name(name)?;
            ensure!(
                !crate::theme::packaged().contains_key(name),
                "Custom theme conflicts with packaged theme: {name}"
            );
            theme.validate()?;
        }
        ensure!(
            self.catalog().contains_key(&self.default_theme),
            "Unknown global theme: {}",
            self.default_theme
        );
        for tool in &self.denied_theme_tools {
            ensure!(
                crate::import::valid_segment(tool),
                "Invalid tool namespace: {tool}"
            );
        }
        if let Some(hook) = &self.filehook {
            crate::filehook::validate(hook)?;
        }
        if let Some(hook) = &self.dirhook {
            crate::dirhook::validate(hook)?;
        }
        crate::alias::validate(&self.sources)?;
        let mut keys = std::collections::BTreeSet::new();
        for source in &self.sources {
            for theme in source.themes.values() {
                theme.validate()?;
            }
            let mut group_keys = BTreeSet::new();
            for group in &source.groups {
                ensure!(
                    !group.key.is_empty()
                        && group
                            .key
                            .iter()
                            .all(|key| crate::import::valid_segment(key)),
                    "Invalid group key"
                );
                ensure!(group_keys.insert(&group.key), "Duplicate group key");
            }
            for key in &source.removed_keys {
                ensure!(
                    !key.is_empty() && key.iter().all(|k| crate::import::valid_segment(k)),
                    "Invalid removed key in {}",
                    source.path.display()
                );
            }
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
                let kinds = usize::from(entry.value.is_some())
                    + usize::from(entry.choices.is_some())
                    + usize::from(entry.script.is_some())
                    + usize::from(!entry.url.is_empty());
                ensure!(
                    kinds == 1,
                    "Node must define exactly one value, list, URL, or script"
                );
                if let Some(value) = &entry.value {
                    crate::template::names(value)?;
                } else if let Some(choices) = &entry.choices {
                    crate::script::validate_choices(choices)?;
                } else if let Some(script) = &entry.script {
                    ensure!(
                        entry.url.is_empty() && entry.args.is_none(),
                        "Resource cannot be both a script and a URL/path"
                    );
                    script.validate()?;
                } else {
                    crate::template::validate_target(&entry.url)?;
                    crate::template::validate_arguments(&entry.url, entry.args.as_deref())?;
                }
                ensure!(
                    keys.insert(entry.key.clone()),
                    "Duplicate key: {}",
                    entry.key.join(" ")
                );
            }
            for (original, alias) in &source.renames {
                ensure!(
                    original.split(' ').all(crate::import::valid_segment)
                        && alias.split(' ').all(crate::import::valid_segment)
                        && original.split(' ').count() == alias.split(' ').count(),
                    "Invalid namespace rename in {}",
                    source.path.display()
                );
            }
        }
        for source in &self.sources {
            for entry in &source.entries {
                if self.version == 1 || entry.is_data() {
                    ensure!(
                        !keys
                            .iter()
                            .any(|key| key.len() > entry.key.len() && key.starts_with(&entry.key)),
                        "Data leaf cannot also be a namespace: {}",
                        entry.key.join(" ")
                    );
                }
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

    pub fn remove(&mut self, key: &[String]) -> Result<()> {
        ensure!(
            !key.is_empty()
                && self.sources.iter().any(|source| source
                    .entries
                    .iter()
                    .any(|entry| entry.key.starts_with(key))),
            "No resource or namespace for: qrlkit {}",
            key.join(" ")
        );
        for source in &mut self.sources {
            if !source
                .entries
                .iter()
                .any(|entry| entry.key.starts_with(key))
            {
                continue;
            }
            let original = crate::import::map_key(key, &source.renames, true);
            source
                .removed_keys
                .retain(|prefix| !prefix.starts_with(&original));
            if !source
                .removed_keys
                .iter()
                .any(|prefix| original.starts_with(prefix))
            {
                source.removed_keys.push(original);
            }
            source.entries.retain(|entry| !entry.key.starts_with(key));
            source.groups.retain(|group| !group.key.starts_with(key));
        }
        // Fully removed imports can be added again and need not exist on disk.
        self.sources.retain(|source| !source.entries.is_empty());
        Ok(())
    }

    pub fn catalog(&self) -> BTreeMap<String, crate::theme::Theme> {
        crate::theme::catalog(&self.custom_themes)
    }

    pub fn global_theme(&self) -> crate::theme::Theme {
        // State loading validates the selected name; fresh state uses terminal.
        self.catalog()
            .remove(&self.default_theme)
            .unwrap_or_default()
    }

    /// Select one complete theme, never a mix of fields from different themes.
    /// The first source containing a merged namespace supplies its tool theme.
    pub fn theme(&self, key: &[String]) -> crate::theme::Theme {
        let Some(tool) = key.first() else {
            return self.global_theme();
        };
        if !self.tool_themes || self.denied_theme_tools.contains(tool) {
            return self.global_theme();
        }
        if let Some(source) = self.sources.iter().find(|source| {
            source
                .entries
                .iter()
                .any(|entry| entry.key.starts_with(key))
        }) {
            let original = crate::import::map_key(key, &source.renames, true);
            for end in (0..=original.len()).rev() {
                if let Some(settings) = source.themes.get(&original[..end].join(" ")) {
                    return settings.clone();
                }
            }
        }
        self.global_theme()
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

    pub fn contains(&self, key: &[String]) -> bool {
        self.sources.iter().any(|source| {
            source
                .entries
                .iter()
                .any(|entry| entry.key.starts_with(key))
        })
    }

    pub fn visible_children(&self, prefix: &[String]) -> Vec<String> {
        self.children(prefix)
            .into_iter()
            .filter(|child| {
                let mut key = prefix.to_vec();
                key.push(child.clone());
                !self
                    .sources
                    .iter()
                    .flat_map(|source| &source.groups)
                    .any(|group| {
                        group.hidden
                            && group.key.len() > prefix.len()
                            && key.starts_with(&group.key)
                    })
            })
            .collect()
    }

    pub fn hint(&self, key: &[String]) -> Option<&str> {
        self.lookup(key)
            .ok()
            .and_then(|entry| entry.hint.as_deref())
            .or_else(|| {
                self.sources
                    .iter()
                    .flat_map(|source| &source.groups)
                    .find(|group| group.key == key)
                    .and_then(|group| group.hint.as_deref())
            })
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
            groups: vec![],
            themes: BTreeMap::new(),
            collision_strategy: None,
            alias: None,
            path: "test.toml".into(),
            renames: BTreeMap::new(),
            removed_keys: vec![],
            entries: [
                vec!["gh", "prs", "b"],
                vec!["gh", "prs", "a"],
                vec!["gh", "repo", "a"],
                vec!["logs", "api"],
            ]
            .into_iter()
            .map(|key| Entry {
                value: None,
                choices: None,
                hint: None,
                browser: None,
                key: key.into_iter().map(String::from).collect(),
                url: "https://example.com".into(),
                script: None,
                args: None,
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
            groups: vec![],
            themes: BTreeMap::new(),
            collision_strategy: None,
            alias: None,
            path: "team.toml".into(),
            renames: BTreeMap::new(),
            removed_keys: vec![],
            entries: vec![Entry {
                value: None,
                choices: None,
                hint: None,
                browser: None,
                key: vec!["git".into(), "prs".into()],
                url: "https://example.com".into(),
                script: None,
                args: None,
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
        fs::write(&path, "version: 3\nbrowser: null\nsources: []").unwrap();
        assert!(State::load(&path).is_err());
    }
}
