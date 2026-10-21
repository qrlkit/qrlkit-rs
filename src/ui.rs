use crate::theme::{Border, Theme};
use anyhow::{Result, bail, ensure};
use ratatui::{
    Terminal, TerminalOptions, Viewport,
    backend::CrosstermBackend,
    crossterm::{
        cursor::{MoveTo, Show},
        event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
    },
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph},
};
use std::{
    cell::RefCell,
    io::{self, IsTerminal},
};

thread_local! {
    static THEME: RefCell<Theme> = RefCell::new(Theme::default());
}

pub fn set_theme(theme: Theme) {
    THEME.with(|current| *current.borrow_mut() = theme);
}

fn style(color: Option<&str>) -> Style {
    Style::default().fg(color
        .and_then(|value| crate::theme::color(value).ok())
        .unwrap_or(Color::Reset))
}

fn option_line(option: &str, selected: bool, theme: &Theme, marker_width: usize) -> Line<'static> {
    let marker = theme.marker();
    let width = Line::raw(marker).width();
    let prefix = if selected {
        format!(
            "{marker}{} ",
            " ".repeat(marker_width.saturating_sub(width))
        )
    } else {
        " ".repeat(marker_width + 1)
    };
    let row_style = if selected {
        let style = style(theme.selected.as_deref().or(theme.text.as_deref()));
        if theme
            .selected
            .as_deref()
            .is_some_and(|color| color != "default")
        {
            style.add_modifier(Modifier::BOLD)
        } else {
            style
        }
    } else {
        style(theme.text.as_deref().or(theme.selected.as_deref()))
    };
    let (key, hint) = option.split_once("        ").unwrap_or((option, ""));
    let mut spans = vec![
        Span::styled(prefix, row_style),
        Span::styled(key.to_owned(), row_style),
    ];
    if !hint.is_empty() {
        spans.push(Span::styled(
            format!("        {hint}"),
            if theme.hint.is_some() {
                style(theme.hint.as_deref())
            } else {
                row_style
            },
        ));
    }
    Line::from(spans)
}

fn draw_prompt(frame: &mut ratatui::Frame, data: &PromptRequest, selected: usize, text: &str) {
    let theme = &data.theme;
    let mut area = frame.area();
    let border = theme.border.unwrap_or_default();
    if border != Border::None {
        let border_type = match border {
            Border::Rounded => BorderType::Rounded,
            Border::Double => BorderType::Double,
            _ => BorderType::Plain,
        };
        let block = Block::bordered()
            .border_type(border_type)
            .border_style(style(theme.border_color.as_deref()))
            .title(Line::styled(
                data.title.clone(),
                style(theme.title.as_deref()),
            ));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        area = inner;
    } else if !data.minimal {
        frame.render_widget(
            Paragraph::new(Line::styled(
                data.title.clone(),
                style(theme.title.as_deref()),
            )),
            Rect::new(area.x, area.y, area.width, area.height.min(1)),
        );
        area.y = area.y.saturating_add(1);
        area.height = area.height.saturating_sub(1);
    }
    if data.input {
        frame.render_widget(
            Paragraph::new(Line::styled(
                format!("{} {text}", theme.marker()),
                style(theme.text.as_deref()),
            )),
            area,
        );
        return;
    }
    let marker_width = std::iter::once(theme)
        .chain(data.row_themes.iter())
        .map(|theme| Line::raw(theme.marker()).width())
        .max()
        .unwrap_or(1);
    let rows = usize::from(area.height);
    let offset = selected.saturating_sub(rows.saturating_sub(1));
    let lines = data
        .options
        .iter()
        .enumerate()
        .skip(offset)
        .take(rows)
        .map(|(i, option)| {
            option_line(
                option,
                i == selected,
                data.row_themes.get(i).unwrap_or(theme),
                marker_width,
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);
}

struct Restore {
    origin: Option<Position>,
}

fn prompt_height(options: usize, input: bool, minimal: bool) -> u16 {
    if input {
        2
    } else if minimal {
        options.max(1) as u16
    } else {
        1 + options.max(1) as u16
    }
}
impl Drop for Restore {
    fn drop(&mut self) {
        // Use the latest viewport origin: drawing may have scrolled or resized it.
        // Clearing alone preserves the last cursor position and leaves blank rows.
        if let Some(origin) = self.origin {
            let _ = execute!(
                io::stderr(),
                MoveTo(origin.x, origin.y),
                Clear(ClearType::FromCursorDown),
                Show
            );
        }
        let _ = disable_raw_mode();
    }
}

fn render_prompt(data: &PromptRequest) -> Result<String> {
    let PromptRequest {
        title,
        options,
        input,
        minimal,
        allow_empty,
        ..
    } = data;
    let (input, minimal, allow_empty) = (*input, *minimal, *allow_empty);
    ensure!(
        io::stdin().is_terminal() && io::stderr().is_terminal(),
        "{title}: an interactive terminal is required"
    );
    let mut restore = Restore { origin: None };
    enable_raw_mode()?;
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(io::stderr()),
        TerminalOptions {
            viewport: Viewport::Inline(if data.theme.border.unwrap_or_default() == Border::None {
                prompt_height(options.len(), input, minimal)
            } else {
                if input {
                    1
                } else {
                    options.len().max(1).min(usize::from(u16::MAX - 2)) as u16
                }
                .saturating_add(2)
            }),
        },
    )?;
    let mut selected = 0usize;
    let mut text = String::new();
    let result = loop {
        terminal.draw(|frame| {
            restore.origin = Some(frame.area().as_position());
            draw_prompt(frame, data, selected, &text);
        })?;
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Esc => break None,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break None,
                KeyCode::Enter if input && (allow_empty || !text.trim().is_empty()) => {
                    break Some(text.trim().into());
                }
                KeyCode::Enter if !input => break Some(selected.to_string()),
                KeyCode::Up | KeyCode::Char('k') if !input => {
                    selected = selected.saturating_sub(1);
                }
                KeyCode::Down | KeyCode::Char('j') if !input => {
                    selected = (selected + 1).min(options.len() - 1);
                }
                KeyCode::Backspace if input => {
                    text.pop();
                }
                KeyCode::Char(c) if input && !c.is_control() => text.push(c),
                _ => {}
            }
        }
    };
    match result {
        Some(result) => Ok(result),
        None => bail!("user cancelled"),
    }
}

#[cfg(test)]
mod tests {
    use super::{key_label, prompt_height};

    #[test]
    fn mixed_markers_align_keys_and_style_hints() {
        use super::*;
        use ratatui::{Terminal, backend::TestBackend};
        let wide = Theme {
            marker: Some("界".into()),
            selected: Some("#abcdef".into()),
            hint: Some("#112233".into()),
            ..Theme::default()
        };
        let long = Theme {
            marker: Some("--x".into()),
            ..Theme::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(40, 2)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Paragraph::new(vec![
                        option_line("repo        My repository", true, &wide, 3),
                        option_line("docs", false, &long, 3),
                    ]),
                    frame.area(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(4, 0)].symbol(), "r");
        assert_eq!(buffer[(4, 1)].symbol(), "d");
        assert_eq!(buffer[(4, 0)].fg, Color::Rgb(171, 205, 239));
        assert_eq!(buffer[(16, 0)].fg, Color::Rgb(17, 34, 51));
        assert_eq!(buffer[(4, 0)].bg, Color::Reset);
    }

    #[test]
    fn borders_titles_and_scrolling_keep_selected_row_visible() {
        use super::*;
        use ratatui::{Terminal, backend::TestBackend};
        for (border, corner) in [
            (Border::Plain, "┌"),
            (Border::Rounded, "╭"),
            (Border::Double, "╔"),
        ] {
            let data = PromptRequest {
                title: "gh".into(),
                options: vec!["home".into(), "issues".into(), "repo".into()],
                input: false,
                minimal: true,
                allow_empty: false,
                theme: Theme {
                    border: Some(border),
                    border_color: Some("#112233".into()),
                    marker: Some("--x".into()),
                    ..Theme::default()
                },
                row_themes: vec![],
            };
            let mut terminal = Terminal::new(TestBackend::new(20, 3)).unwrap();
            terminal
                .draw(|frame| draw_prompt(frame, &data, 2, ""))
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert_eq!(buffer[(0, 0)].symbol(), corner);
            assert_eq!(buffer[(0, 0)].fg, Color::Rgb(17, 34, 51));
            assert_eq!(buffer[(1, 0)].symbol(), "g");
            assert_eq!(buffer[(1, 1)].symbol(), "-");
            assert_eq!(buffer[(5, 1)].symbol(), "r");
            // Tiny terminals must not panic, even when borders consume all space.
            let mut tiny = Terminal::new(TestBackend::new(1, 1)).unwrap();
            tiny.draw(|frame| draw_prompt(frame, &data, 2, "")).unwrap();
        }
        let old = "title: Test\noptions: [repo]\ninput: false\nminimal: true\n";
        let data: PromptRequest = serde_yaml_ng::from_str(old).unwrap();
        assert_eq!(data.theme, Theme::default());
        assert!(data.row_themes.is_empty());
    }

    #[test]
    fn hint_labels_preserve_keys_and_stay_on_one_row() {
        assert_eq!(
            key_label("repo", Some("Opens repo")),
            "repo        Opens repo"
        );
        assert_eq!(key_label("repo", None), "repo");
        assert_eq!(key_label("repo", Some("  ")), "repo");
        assert_eq!(
            key_label("repo", Some("One\nTwo\tThree")),
            "repo        One Two Three"
        );
    }

    #[test]
    fn picker_height_includes_every_option() {
        assert_eq!(prompt_height(0, false, true), 1);
        assert_eq!(prompt_height(7, false, true), 7);
        assert_eq!(prompt_height(7, false, false), 8);
        assert_eq!(prompt_height(7, true, false), 2);
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct PromptRequest {
    title: String,
    options: Vec<String>,
    input: bool,
    minimal: bool,
    #[serde(default)]
    allow_empty: bool,
    #[serde(default)]
    theme: Theme,
    #[serde(default)]
    row_themes: Vec<Theme>,
}

// Crossterm sends cursor queries to stdout. Isolate the prompt in a child whose
// stdout and stderr both point at the terminal, preserving the parent's stdout.
fn prompt(
    title: &str,
    options: &[String],
    input: bool,
    minimal: bool,
    allow_empty: bool,
) -> Result<String> {
    let data = PromptRequest {
        title: title.into(),
        options: options.to_vec(),
        input,
        minimal,
        allow_empty,
        theme: THEME.with(|theme| theme.borrow().clone()),
        row_themes: vec![],
    };
    send_prompt(data)
}

fn send_prompt(data: PromptRequest) -> Result<String> {
    ensure!(
        io::stdin().is_terminal() && io::stderr().is_terminal(),
        "{}: an interactive terminal is required",
        data.title
    );
    let request = tempfile::NamedTempFile::new()?;
    let response = tempfile::NamedTempFile::new()?;
    std::fs::write(request.path(), serde_yaml_ng::to_string(&data)?)?;
    let status = std::process::Command::new(std::env::current_exe()?)
        .arg("__prompt")
        .arg(request.path())
        .arg(response.path())
        .stdout(std::process::Stdio::from(io::stderr()))
        .status()?;
    ensure!(status.success(), "Terminal prompt failed");
    let result: std::result::Result<String, String> =
        serde_yaml_ng::from_str(&std::fs::read_to_string(response.path())?)?;
    result.map_err(anyhow::Error::msg)
}

pub fn run_prompt(request: &std::path::Path, response: &std::path::Path) -> Result<()> {
    let data: PromptRequest = serde_yaml_ng::from_str(&std::fs::read_to_string(request)?)?;
    data.theme.validate()?;
    for theme in &data.row_themes {
        theme.validate()?;
    }
    ensure!(
        data.input || !data.options.is_empty(),
        "No options available"
    );
    let result = render_prompt(&data).map_err(|error| error.to_string());
    std::fs::write(response, serde_yaml_ng::to_string(&result)?)?;
    Ok(())
}

pub fn select(title: &str, options: &[String]) -> Result<usize> {
    ensure!(!options.is_empty(), "No options available");
    Ok(prompt(title, options, false, false, false)?.parse()?)
}

pub fn input(title: &str) -> Result<String> {
    prompt(title, &[], true, false, false)
}

/// Input where pressing Enter accepts the caller's default.
pub fn input_optional(title: &str) -> Result<String> {
    prompt(title, &[], true, false, true)
}

/// Keep hints on one terminal row, including when loaded from hand-edited state.
pub fn key_label(key: &str, hint: Option<&str>) -> String {
    match hint.filter(|hint| !hint.trim().is_empty()) {
        Some(hint) => {
            let hint: String = hint
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            format!("{key}        {hint}")
        }
        None => key.to_owned(),
    }
}

/// Resource navigation only: keys, optional hints, and a selection marker.
pub fn select_key(
    title: &str,
    options: &[String],
    theme: Theme,
    row_themes: Vec<Theme>,
) -> Result<usize> {
    ensure!(!options.is_empty(), "No keys available");
    Ok(send_prompt(PromptRequest {
        title: title.to_owned(),
        options: options.to_vec(),
        input: false,
        minimal: true,
        allow_empty: false,
        theme,
        row_themes,
    })?
    .parse()?)
}
