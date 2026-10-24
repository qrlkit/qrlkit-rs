use crate::format::Node;
use anyhow::{Context, Result, bail, ensure};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Border {
    #[default]
    None,
    Plain,
    Rounded,
    Double,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SelectionWidth {
    #[default]
    Row,
    Content,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_background: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_width: Option<SelectionWidth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<Border>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
}

pub fn color(value: &str) -> Result<Color> {
    if value == "default" {
        return Ok(Color::Reset);
    }
    let hex = value.strip_prefix('#').unwrap_or("");
    ensure!(
        hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
        "Theme colors must be #RRGGBB or default: {value}"
    );
    let rgb = u32::from_str_radix(hex, 16)?;
    Ok(Color::Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8))
}

impl Theme {
    pub fn marker(&self) -> &str {
        self.marker.as_deref().unwrap_or(">")
    }

    pub fn validate(&self) -> Result<()> {
        for value in [
            &self.background,
            &self.selected_background,
            &self.selected,
            &self.text,
            &self.hint,
            &self.title,
            &self.border_color,
        ]
        .into_iter()
        .flatten()
        {
            color(value)?;
        }
        if let Some(marker) = &self.marker {
            ensure!(
                !marker.chars().any(char::is_control),
                "Theme marker cannot contain control characters"
            );
            ensure!(
                marker.chars().count() <= 16,
                "Theme marker must be at most 16 characters"
            );
        }
        Ok(())
    }

    fn parse(node: Node) -> Result<Self> {
        let theme = match node {
            Node::String(name) => named(&name)?,
            Node::Table(table) => {
                let values = table
                    .into_iter()
                    .map(|(key, value)| {
                        Ok((
                            key,
                            value
                                .as_str()
                                .context("Theme values must be strings")?
                                .to_owned(),
                        ))
                    })
                    .collect::<Result<BTreeMap<_, _>>>()?;
                serde_json::from_value(serde_json::to_value(values)?)?
            }
            _ => bail!("theme must be a name or an object"),
        };
        theme.validate()?;
        Ok(theme)
    }
}

/// Remove reserved metadata before flattening resources. Paths stay in source
/// coordinates so namespace renames and scoped aliases can share the same data.
pub fn extract(
    node: &mut Node,
    key: &mut Vec<String>,
    themes: &mut BTreeMap<String, Theme>,
) -> Result<()> {
    let Node::Table(table) = node else {
        return Ok(());
    };
    if let Some(value) = table.remove("theme") {
        themes.insert(
            key.join(" "),
            Theme::parse(value).with_context(|| format!("Theme at {}", key.join(" ")))?,
        );
    }
    if table.contains_key("run") {
        return Ok(());
    }
    for (name, child) in table {
        key.push(name.clone());
        extract(child, key, themes)?;
        key.pop();
    }
    Ok(())
}

/// The same strict file format is used for packaged and personal themes.
pub fn read_file(text: &str) -> Result<(String, Theme)> {
    let Node::Table(mut root) = crate::format::parse(std::path::Path::new("theme.toml"), text)?
    else {
        unreachable!()
    };
    let Node::Table(mut table) = root
        .remove("theme")
        .context("Theme file requires [theme]")?
    else {
        bail!("theme must be a table");
    };
    ensure!(
        root.is_empty(),
        "Theme file accepts only [theme]; name belongs inside [theme]"
    );
    let name = table.remove("name").context("theme.name is required")?;
    let name = name
        .as_str()
        .context("theme.name must be a string")?
        .to_owned();
    valid_name(&name)?;
    Ok((name, Theme::parse(Node::Table(table))?))
}

pub fn valid_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 64
            && name
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_')),
        "Theme name must be 1–64 lowercase letters, digits, hyphens, or underscores, starting with a letter or digit"
    );
    Ok(())
}

pub const FILES: &[(&str, &str)] = &[
    ("terminal", include_str!("../themes/terminal.toml")),
    ("dracula", include_str!("../themes/dracula.toml")),
    (
        "catppuccin-mocha",
        include_str!("../themes/catppuccin-mocha.toml"),
    ),
    (
        "catppuccin-latte",
        include_str!("../themes/catppuccin-latte.toml"),
    ),
    ("tokyo-night", include_str!("../themes/tokyo-night.toml")),
    ("gruvbox-dark", include_str!("../themes/gruvbox-dark.toml")),
    (
        "gruvbox-light",
        include_str!("../themes/gruvbox-light.toml"),
    ),
    ("nord", include_str!("../themes/nord.toml")),
];

pub fn packaged() -> &'static BTreeMap<String, Theme> {
    static THEMES: std::sync::OnceLock<BTreeMap<String, Theme>> = std::sync::OnceLock::new();
    THEMES.get_or_init(|| {
        FILES
            .iter()
            .map(|(expected, text)| {
                let (name, theme) =
                    read_file(text).expect("packaged theme must pass catalog tests");
                assert_eq!(&name, expected, "packaged theme name must match its file");
                (name, theme)
            })
            .collect()
    })
}

pub fn named(name: &str) -> Result<Theme> {
    packaged().get(name).cloned().with_context(|| {
        format!("Unknown theme: {name}; run qrlkit themes to list available themes")
    })
}

pub fn catalog(custom: &BTreeMap<String, Theme>) -> BTreeMap<String, Theme> {
    let mut themes = packaged().clone();
    themes.extend(custom.clone());
    themes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{import, store::State};

    #[test]
    fn packaged_themes_are_unique_complete_valid_and_use_the_import_format() {
        let mut names = std::collections::BTreeSet::new();
        for (expected, text) in FILES {
            let (name, theme) = read_file(text).unwrap();
            assert_eq!(&name, expected);
            assert!(names.insert(name.clone()));
            assert_eq!(named(&name).unwrap(), theme);
            for field in [
                &theme.background,
                &theme.selected_background,
                &theme.selected,
                &theme.text,
                &theme.hint,
                &theme.title,
                &theme.border_color,
                &theme.marker,
            ] {
                assert!(field.is_some(), "Incomplete packaged theme: {name}");
            }
            assert!(theme.border.is_some());
            assert!(theme.selection_width.is_some());
            theme.validate().unwrap();
        }
        for removed in ["ocean", "forest", "rose"] {
            assert!(named(removed).is_err());
        }
    }

    #[test]
    fn namespaces_replace_whole_themes_across_formats_and_follow_renames_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        for (extension, text) in [
            (
                "toml",
                "theme = { marker = '--root', background = '#112233' }\n[gh]\ntheme = 'dracula'\nhome = 'https://github.com'\n[gh.work]\ntheme = { selected = '#abcdef' }\nrepo = 'https://github.com/example'\n[qk]\ndocs = 'https://example.com'",
            ),
            (
                "yaml",
                "theme: {marker: --root, background: '#112233'}\ngh:\n  theme: dracula\n  home: https://github.com\n  work:\n    theme: {selected: '#abcdef'}\n    repo: https://github.com/example\nqk:\n  docs: https://example.com",
            ),
            (
                "json",
                r##"{"theme":{"marker":"--root","background":"#112233"},"gh":{"theme":"dracula","home":"https://github.com","work":{"theme":{"selected":"#abcdef"},"repo":"https://github.com/example"}},"qk":{"docs":"https://example.com"}}"##,
            ),
        ] {
            let path = dir.path().join(format!("resources.{extension}"));
            std::fs::write(&path, text).unwrap();
            let mut state = State {
                default_theme: "nord".into(),
                sources: vec![import::read(&path, BTreeMap::new()).unwrap()],
                ..State::default()
            };
            assert_eq!(state.children(&[]), ["gh", "qk"]);
            assert_eq!(state.theme(&[]), named("nord").unwrap());
            assert_eq!(
                state.theme(&["gh".into(), "home".into()]),
                named("dracula").unwrap()
            );
            let partial = Theme {
                selected: Some("#abcdef".into()),
                ..Theme::default()
            };
            assert_eq!(
                state.theme(&["gh".into(), "work".into(), "repo".into()]),
                partial
            );
            assert_eq!(partial.marker(), ">");
            assert_eq!(partial.border.unwrap_or_default(), Border::None);
            assert_eq!(
                partial.selection_width.unwrap_or_default(),
                SelectionWidth::Row
            );
            assert!(
                partial.text.is_none() && partial.background.is_none() && partial.hint.is_none()
            );
            assert_eq!(state.theme(&["qk".into()]).marker(), "--root");
            import::rename(&mut state.sources[0], "gh", "github").unwrap();
            assert_eq!(state.theme(&["github".into()]), named("dracula").unwrap());
            assert_eq!(state.theme(&["github".into(), "work".into()]), partial);
            std::fs::write(&path, text.replace("dracula", "nord")).unwrap();
            state.sources = import::reload(&state.sources).unwrap();
            assert_eq!(state.theme(&["github".into()]), named("nord").unwrap());
            let saved = dir.path().join("state.yaml");
            state.save(&saved).unwrap();
            assert_eq!(
                State::load(&saved)
                    .unwrap()
                    .theme(&["github".into(), "work".into()]),
                partial
            );
        }
    }

    #[test]
    fn policy_applies_to_all_files_and_descendants_without_affecting_other_tools() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = State {
            default_theme: "nord".into(),
            ..State::default()
        };
        for (file, text) in [
            (
                "a.toml",
                "[gh]\ntheme = 'dracula'\n[gh.work]\ntheme = { marker = '--x' }\nrepo = 'https://example.com'\n[qk]\ntheme = 'gruvbox-dark'\ndocs = 'https://example.com'",
            ),
            (
                "b.toml",
                "[gh]\ntheme = 'catppuccin-mocha'\nother = 'https://example.com'\n[plain]\ndocs = 'https://example.com'",
            ),
        ] {
            let path = dir.path().join(file);
            std::fs::write(&path, text).unwrap();
            state
                .sources
                .push(import::read(&path, BTreeMap::new()).unwrap());
        }
        let global = state.global_theme();
        assert_eq!(state.theme(&["plain".into()]), global);
        assert_eq!(state.theme(&["gh".into()]), named("dracula").unwrap()); // First source wins a merged namespace
        state.denied_theme_tools.insert("gh".into());
        for path in [
            vec!["gh"],
            vec!["gh", "work"],
            vec!["gh", "work", "repo"],
            vec!["gh", "other"],
        ] {
            assert_eq!(
                state.theme(&path.into_iter().map(String::from).collect::<Vec<_>>()),
                global
            );
        }
        assert_eq!(state.theme(&["qk".into()]), named("gruvbox-dark").unwrap());
        state.tool_themes = false;
        assert_eq!(state.theme(&["qk".into()]), global);
        state.tool_themes = true;
        assert_eq!(state.theme(&["gh".into(), "other".into()]), global);
        state.denied_theme_tools.remove("gh");
        assert_eq!(
            state.theme(&["gh".into(), "other".into()]),
            named("catppuccin-mocha").unwrap()
        );
        // Resource files cannot supply their own permission settings inside a theme.
        assert!(read_file("[theme]\nname = 'bad'\ntool_themes = 'on'").is_err());
    }

    #[test]
    fn invalid_theme_fields_are_rejected_and_script_environment_is_not_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resources.toml");
        for value in [
            "'unknown'",
            "'ocean'",
            "{selected = 'purple'}",
            "{border = 'dotted'}",
            "{unknown = 'value'}",
            "{marker = \"a\\nb\"}",
            "{selected = '#１２３'}",
            "{selected = ['#ffffff']}",
            "{marker = '12345678901234567'}",
            "{background = 'bad'}",
            "{selected_background = 'bad'}",
            "{selection_width = 'bad'}",
        ] {
            std::fs::write(
                &path,
                format!("[gh]\ntheme = {value}\nhome = 'https://github.com'"),
            )
            .unwrap();
            assert!(import::read(&path, BTreeMap::new()).is_err(), "{value}");
        }
        std::fs::write(&path, "[task]\ntheme = 'dracula'\nrun = 'echo hello'\n[task.env]\ntheme = 'ordinary environment value'").unwrap();
        assert!(import::read(&path, BTreeMap::new()).is_ok());
        assert_eq!(color("default").unwrap(), Color::Reset);
    }
}
