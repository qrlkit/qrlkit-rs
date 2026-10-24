use crate::theme::{Border, SelectionWidth, Theme};
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

/// Render previews with the actual picker, including its box and background.
pub fn print_theme_preview(theme: &Theme) -> Result<()> {
    use ratatui::{
        backend::TestBackend,
        crossterm::style::{Color as TerminalColor, Stylize, style as terminal_style},
    };
    let colors = io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none_or(|value| value.is_empty());
    let width = if io::stdout().is_terminal() {
        ratatui::crossterm::terminal::size()
            .map(|(width, _)| width.min(48))
            .unwrap_or(48)
    } else {
        48
    }
    .max(1);
    let data = PromptRequest {
        title: "Preview".into(),
        options: vec![
            "repo        Open repository".into(),
            "issues        Browse issues".into(),
        ],
        input: false,
        minimal: true,
        allow_empty: false,
        theme: theme.clone(),
        row_themes: vec![],
        live_themes: vec![],
        initial_selection: 0,
    };
    let mut terminal = Terminal::new(TestBackend::new(width, 4))?;
    terminal.draw(|frame| draw_prompt(frame, &data, 0, ""))?;
    let buffer = terminal.backend().buffer();
    let terminal_color = |color| match color {
        Color::Rgb(r, g, b) => TerminalColor::Rgb { r, g, b },
        _ => TerminalColor::Reset,
    };
    for y in 0..4 {
        let mut x = 0;
        while x < width {
            let cell = &buffer[(x, y)];
            let style = cell.style();
            let mut text = String::new();
            while x < width && buffer[(x, y)].style() == style {
                let symbol = buffer[(x, y)].symbol();
                text.push_str(symbol);
                x = x.saturating_add(Line::raw(symbol).width().max(1) as u16);
            }
            if colors {
                let mut content = terminal_style(text)
                    .with(terminal_color(cell.fg))
                    .on(terminal_color(cell.bg));
                if cell.modifier.contains(Modifier::BOLD) {
                    content = content.bold();
                }
                print!("{content}");
            } else {
                print!("{text}");
            }
        }
        println!();
    }
    println!();
    Ok(())
}

fn style(color: Option<&str>) -> Style {
    Style::default().fg(color
        .and_then(|value| crate::theme::color(value).ok())
        .unwrap_or(Color::Reset))
}

fn background_style(color: Option<&str>) -> Style {
    match color.and_then(|value| crate::theme::color(value).ok()) {
        Some(color) => Style::default().bg(color),
        None => Style::default(),
    }
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
    let mut row_style = if selected {
        let style = style(theme.selected.as_deref());
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
        style(theme.text.as_deref())
    };
    if selected {
        row_style = row_style.patch(background_style(Some(
            theme.selected_background.as_deref().unwrap_or("default"),
        )));
    }
    let (key, hint) = option.split_once("        ").unwrap_or((option, ""));
    let mut spans = vec![
        Span::styled(prefix, row_style),
        Span::styled(key.to_owned(), row_style),
    ];
    if !hint.is_empty() {
        spans.push(Span::styled(
            format!("        {hint}"),
            row_style.patch(style(theme.hint.as_deref())),
        ));
    }
    Line::from(spans)
}

fn draw_prompt(frame: &mut ratatui::Frame, data: &PromptRequest, selected: usize, text: &str) {
    let live = !data.live_themes.is_empty();
    let theme = data.live_themes.get(selected).unwrap_or(&data.theme);
    let title = if live {
        format!(
            "Choose theme: {} (Enter saves, Esc cancels)",
            data.options[selected]
        )
    } else {
        data.title.clone()
    };
    let mut area = frame.area();
    frame.render_widget(
        Block::default().style(background_style(theme.background.as_deref())),
        area,
    );
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
            .title(Line::styled(title.clone(), style(theme.title.as_deref())));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        area = inner;
    } else if !data.minimal {
        frame.render_widget(
            Paragraph::new(Line::styled(title.clone(), style(theme.title.as_deref()))),
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
    for (row, (i, option)) in data
        .options
        .iter()
        .enumerate()
        .skip(offset)
        .take(rows)
        .enumerate()
    {
        let row_theme = if live {
            theme
        } else {
            data.row_themes.get(i).unwrap_or(theme)
        };
        let selected = i == selected;
        let line = option_line(option, selected, row_theme, marker_width);
        let mut paragraph = Paragraph::new(line);
        if selected && row_theme.selection_width.unwrap_or_default() == SelectionWidth::Row {
            paragraph = paragraph.style(background_style(Some(
                row_theme
                    .selected_background
                    .as_deref()
                    .unwrap_or("default"),
            )));
        }
        frame.render_widget(
            paragraph,
            Rect::new(area.x, area.y + row as u16, area.width, 1),
        );
    }
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
                ratatui::crossterm::style::ResetColor,
                ratatui::crossterm::style::SetAttribute(
                    ratatui::crossterm::style::Attribute::Reset
                ),
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
            viewport: Viewport::Inline(if !data.live_themes.is_empty() {
                (options.len().min(usize::from(u16::MAX - 2)) as u16).saturating_add(2)
            } else if data.theme.border.unwrap_or_default() == Border::None {
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
    let mut selected = data.initial_selection;
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
                KeyCode::Esc if !data.live_themes.is_empty() => break Some(String::new()),
                KeyCode::Esc => break None,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    break if data.live_themes.is_empty() {
                        None
                    } else {
                        Some(String::new())
                    };
                }
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
    fn live_picker_replaces_the_entire_appearance_and_resets_missing_values() {
        use super::*;
        use ratatui::{Terminal, backend::TestBackend};
        let data = PromptRequest {
            title: "Choose theme".into(),
            options: vec!["terminal".into(), "personal".into()],
            input: false,
            minimal: false,
            allow_empty: false,
            theme: Theme {
                background: Some("#ff0000".into()),
                ..Theme::default()
            },
            row_themes: vec![Theme {
                selected: Some("#ff0000".into()),
                ..Theme::default()
            }],
            live_themes: vec![
                Theme::default(),
                Theme {
                    background: Some("#101112".into()),
                    text: Some("#112233".into()),
                    selected: Some("#ffffff".into()),
                    selected_background: Some("#202122".into()),
                    border: Some(Border::Double),
                    marker: Some("--x".into()),
                    selection_width: Some(SelectionWidth::Content),
                    ..Theme::default()
                },
            ],
            initial_selection: 1,
        };
        let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
        terminal
            .draw(|frame| draw_prompt(frame, &data, 1, ""))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), "╔");
        assert_eq!(buffer[(1, 2)].symbol(), "-");
        assert_eq!(buffer[(5, 2)].symbol(), "p");
        assert_eq!(buffer[(5, 2)].bg, Color::Rgb(32, 33, 34));
        assert_eq!(buffer[(30, 2)].bg, Color::Rgb(16, 17, 18));
        assert_eq!(buffer[(5, 1)].fg, Color::Rgb(17, 34, 51));
        terminal
            .draw(|frame| draw_prompt(frame, &data, 0, ""))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 1)].symbol(), ">");
        assert_eq!(buffer[(0, 0)].symbol(), "C");
        assert_eq!(buffer[(30, 2)].bg, Color::Reset);
        assert_eq!(buffer[(2, 1)].fg, Color::Reset);
        for (width, height) in [(1, 1), (12, 3)] {
            let mut tiny = Terminal::new(TestBackend::new(width, height)).unwrap();
            tiny.draw(|frame| draw_prompt(frame, &data, 1, "")).unwrap();
        }
        // Missing selection colors use terminal defaults, even with a custom base.
        let mut partial = data;
        partial.live_themes[1].selected_background = None;
        partial.live_themes[1].selected = None;
        terminal
            .draw(|frame| draw_prompt(frame, &partial, 1, ""))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(5, 2)].bg, Color::Reset);
        assert_eq!(terminal.backend().buffer()[(5, 2)].fg, Color::Reset);
    }

    #[test]
    fn backgrounds_fill_the_box_and_selection_respects_width_and_parent() {
        use super::*;
        use ratatui::{Terminal, backend::TestBackend};
        for width in [
            None,
            Some(SelectionWidth::Row),
            Some(SelectionWidth::Content),
        ] {
            let theme = Theme {
                background: Some("#010203".into()),
                selected_background: Some("#040506".into()),
                selection_width: width,
                border: Some(Border::Rounded),
                hint: Some("#abcdef".into()),
                ..Theme::default()
            };
            let child = Theme {
                background: Some("#ff0000".into()),
                ..theme.clone()
            };
            let mut data = PromptRequest {
                live_themes: vec![],
                initial_selection: 0,
                title: "gh".into(),
                options: vec!["repo        Hint".into(), "docs".into()],
                input: false,
                minimal: true,
                allow_empty: false,
                theme,
                row_themes: vec![child.clone(), child],
            };
            let mut terminal = Terminal::new(TestBackend::new(30, 5)).unwrap();
            terminal
                .draw(|frame| draw_prompt(frame, &data, 0, ""))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let base = Color::Rgb(1, 2, 3);
            let selection = Color::Rgb(4, 5, 6);
            assert_eq!(buffer[(0, 1)].bg, base); // Border
            assert_eq!(buffer[(1, 0)].bg, base); // Title
            assert_eq!(buffer[(3, 2)].bg, base); // Child background stays out of parent
            assert_eq!(buffer[(25, 3)].bg, base); // Empty space
            assert_eq!(buffer[(3, 1)].bg, selection);
            assert_eq!(buffer[(15, 1)].bg, selection); // Hint retains selection background
            assert_eq!(buffer[(15, 1)].fg, Color::Rgb(171, 205, 239));
            assert_eq!(
                buffer[(25, 1)].bg,
                if width == Some(SelectionWidth::Content) {
                    base
                } else {
                    selection
                }
            );
            terminal
                .draw(|frame| draw_prompt(frame, &data, 1, ""))
                .unwrap();
            assert_eq!(terminal.backend().buffer()[(3, 1)].bg, base);
            data.row_themes[1].selected_background = Some("default".into());
            terminal
                .draw(|frame| draw_prompt(frame, &data, 1, ""))
                .unwrap();
            assert_eq!(terminal.backend().buffer()[(3, 2)].bg, Color::Reset);
            data.input = true;
            terminal
                .draw(|frame| draw_prompt(frame, &data, 0, "hello"))
                .unwrap();
            assert_eq!(terminal.backend().buffer()[(3, 1)].bg, base);
        }
    }

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
                live_themes: vec![],
                initial_selection: 0,
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
    #[serde(default)]
    live_themes: Vec<Theme>,
    #[serde(default)]
    initial_selection: usize,
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
        live_themes: vec![],
        initial_selection: 0,
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
    for theme in &data.live_themes {
        theme.validate()?;
    }
    ensure!(
        data.live_themes.is_empty() || data.live_themes.len() == data.options.len(),
        "Theme previews must match the options"
    );
    ensure!(
        data.input || data.initial_selection < data.options.len(),
        "Invalid initial selection"
    );
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
        live_themes: vec![],
        initial_selection: 0,
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

/// The child's response is only committed after Enter; cancellation saves nothing.
pub fn select_theme(
    names: &[String],
    themes: Vec<Theme>,
    selected: usize,
) -> Result<Option<usize>> {
    ensure!(
        !names.is_empty() && names.len() == themes.len() && selected < names.len(),
        "Invalid theme catalog"
    );
    let response = send_prompt(PromptRequest {
        title: "Choose theme".into(),
        options: names.to_vec(),
        input: false,
        minimal: false,
        allow_empty: false,
        theme: Theme::default(),
        row_themes: vec![],
        live_themes: themes,
        initial_selection: selected,
    })?;
    if response.is_empty() {
        Ok(None)
    } else {
        Ok(Some(response.parse()?))
    }
}
