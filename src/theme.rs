use crate::format::Node;
use anyhow::{Context, Result, bail, ensure};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum NamedTheme {
    #[default]
    Terminal,
    Ocean,
    Forest,
    Rose,
}

impl NamedTheme {
    pub const ALL: [Self; 4] = [Self::Terminal, Self::Ocean, Self::Forest, Self::Rose];

    pub fn name(self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Ocean => "ocean",
            Self::Forest => "forest",
            Self::Rose => "rose",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal default colors",
            Self::Ocean => "Blue/cyan (#88c0d0)",
            Self::Forest => "Green (#a3be8c)",
            Self::Rose => "Pink (#eba0ac)",
        }
    }

    pub fn theme(self) -> Theme {
        let accent = match self {
            Self::Terminal => "default",
            Self::Ocean => "#88c0d0",
            Self::Forest => "#a3be8c",
            Self::Rose => "#eba0ac",
        };
        Theme {
            selected: Some(accent.into()),
            title: Some(accent.into()),
            ..Theme::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Border {
    #[default]
    None,
    Plain,
    Rounded,
    Double,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
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
    pub fn overlay(&mut self, other: &Self) {
        macro_rules! merge {
            ($($field:ident),*) => {$(if other.$field.is_some() { self.$field = other.$field.clone(); })*};
        }
        merge!(selected, text, hint, title, border_color, marker);
        if other.border.is_some() {
            self.border = other.border;
        }
    }

    pub fn marker(&self) -> &str {
        self.marker.as_deref().unwrap_or(">")
    }

    pub fn validate(&self) -> Result<()> {
        for value in [
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
            Node::String(name) => NamedTheme::ALL
                .into_iter()
                .find(|theme| theme.name() == name)
                .with_context(|| {
                    format!("Unknown theme: {name}; use terminal, ocean, forest, or rose")
                })?
                .theme(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{import, store::State};

    #[test]
    fn namespaces_inherit_override_rename_and_reload_across_formats() {
        let dir = tempfile::tempdir().unwrap();
        for (extension, text) in [
            (
                "toml",
                "theme = { border = 'rounded', marker = '--x', text = '#112233' }\n[gh]\ntheme = 'ocean'\nhome = 'https://github.com'\n[gh.work]\ntheme = { selected = '#abcdef', border = 'none' }\nrepo = 'https://github.com/example'\n[qk]\ntheme = 'rose'\ndocs = 'https://example.com'",
            ),
            (
                "yaml",
                "theme: {border: rounded, marker: --x, text: '#112233'}\ngh:\n  theme: ocean\n  home: https://github.com\n  work:\n    theme: {selected: '#abcdef', border: none}\n    repo: https://github.com/example\nqk:\n  theme: rose\n  docs: https://example.com",
            ),
            (
                "json",
                r##"{"theme":{"border":"rounded","marker":"--x","text":"#112233"},"gh":{"theme":"ocean","home":"https://github.com","work":{"theme":{"selected":"#abcdef","border":"none"},"repo":"https://github.com/example"}},"qk":{"theme":"rose","docs":"https://example.com"}}"##,
            ),
        ] {
            let path = dir.path().join(format!("resources.{extension}"));
            std::fs::write(&path, text).unwrap();
            let source = import::read(&path, BTreeMap::new()).unwrap();
            assert_eq!(source.entries.len(), 3);
            let mut state = State {
                sources: vec![source],
                ..State::default()
            };
            assert_eq!(state.children(&[]), ["gh", "qk"]);
            let theme = state.theme(&["gh".into(), "home".into()]);
            assert_eq!(theme.selected.as_deref(), Some("#88c0d0"));
            assert_eq!(theme.text.as_deref(), Some("#112233"));
            assert_eq!(theme.border, Some(Border::Rounded));
            assert_eq!(theme.marker(), "--x");
            let nested = state.theme(&["gh".into(), "work".into()]);
            assert_eq!(nested.selected.as_deref(), Some("#abcdef"));
            assert_eq!(nested.border, Some(Border::None));
            assert_eq!(nested.marker(), "--x");
            assert_eq!(
                state.theme(&["qk".into()]).selected.as_deref(),
                Some("#eba0ac")
            );
            import::rename(&mut state.sources[0], "gh", "github").unwrap();
            assert_eq!(state.theme(&["github".into(), "home".into()]), theme);
            let saved = dir.path().join("state.yaml");
            state.save(&saved).unwrap();
            let mut state = State::load(&saved).unwrap();
            assert_eq!(state.theme(&["github".into()]), theme);
            std::fs::write(&path, text.replace("ocean", "forest")).unwrap();
            state.sources = import::reload(&state.sources).unwrap();
            assert_eq!(
                state.theme(&["github".into()]).selected.as_deref(),
                Some("#a3be8c")
            );
        }
    }

    #[test]
    fn saved_default_is_inherited_and_namespace_settings_take_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resources.toml");
        std::fs::write(&path, "theme = { border = 'rounded' }\n[gh]\nhome = 'https://github.com'\n[gh.work]\ntheme = { marker = '--x' }\nrepo = 'https://github.com/example'\n[qk]\ntheme = 'rose'\ndocs = 'https://example.com'").unwrap();
        let state = State {
            default_theme: NamedTheme::Forest,
            sources: vec![import::read(&path, BTreeMap::new()).unwrap()],
            ..State::default()
        };
        assert_eq!(state.theme(&[]).selected.as_deref(), Some("#a3be8c"));
        let theme = state.theme(&["gh".into(), "work".into()]);
        assert_eq!(theme.selected.as_deref(), Some("#a3be8c"));
        assert_eq!(theme.border, Some(Border::Rounded));
        assert_eq!(theme.marker(), "--x");
        assert_eq!(
            state.theme(&["qk".into()]).selected.as_deref(),
            Some("#eba0ac")
        );
        let saved = dir.path().join("state.yaml");
        state.save(&saved).unwrap();
        assert_eq!(
            State::load(&saved)
                .unwrap()
                .theme(&["gh".into(), "work".into()]),
            theme
        );
    }

    #[test]
    fn invalid_settings_are_rejected_and_script_environment_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("resources.toml");
        for value in [
            "'unknown'",
            "{selected = 'purple'}",
            "{border = 'dotted'}",
            "{unknown = 'value'}",
            "{marker = \"a\\nb\"}",
            "{selected = '#１２３'}",
            "{selected = ['#ffffff']}",
            "{marker = '12345678901234567'}",
        ] {
            std::fs::write(
                &path,
                format!("[gh]\ntheme = {value}\nhome = 'https://github.com'"),
            )
            .unwrap();
            assert!(import::read(&path, BTreeMap::new()).is_err(), "{value}");
        }
        std::fs::write(&path, "[task]\ntheme = 'ocean'\nrun = 'echo hello'\n[task.env]\ntheme = 'ordinary environment value'").unwrap();
        assert!(import::read(&path, BTreeMap::new()).is_ok());
        assert_eq!(color("default").unwrap(), Color::Reset);
    }
}
