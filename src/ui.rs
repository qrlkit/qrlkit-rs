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
    layout::Position,
    widgets::Paragraph,
};
use std::io::{self, IsTerminal};

struct Restore {
    origin: Option<Position>,
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

fn render_prompt(
    title: &str,
    options: &[String],
    input: bool,
    minimal: bool,
    allow_empty: bool,
) -> Result<String> {
    ensure!(
        io::stdin().is_terminal() && io::stderr().is_terminal(),
        "{title}: an interactive terminal is required"
    );
    let mut restore = Restore { origin: None };
    enable_raw_mode()?;
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(io::stderr()),
        TerminalOptions {
            viewport: Viewport::Inline(if minimal {
                options.len().clamp(1, 4) as u16
            } else {
                6
            }),
        },
    )?;
    let mut selected = 0usize;
    let mut text = String::new();
    let result = loop {
        terminal.draw(|frame| {
            restore.origin = Some(frame.area().as_position());
            let mut lines = if minimal {
                vec![]
            } else {
                vec![title.to_owned()]
            };
            if input {
                lines.push(format!("> {text}"));
            } else {
                let rows = if minimal {
                    usize::from(frame.area().height).max(1)
                } else {
                    4
                };
                let start = selected.saturating_sub(rows - 1);
                for (i, option) in options.iter().enumerate().skip(start).take(rows) {
                    lines.push(format!(
                        "{} {}",
                        if i == selected { ">" } else { " " },
                        option
                    ));
                }
            }
            if !minimal {
                lines.push(
                    if input {
                        "Enter confirm · Esc cancel"
                    } else {
                        "↑/↓ or j/k select · Enter confirm · Esc cancel"
                    }
                    .into(),
                );
            }
            frame.render_widget(Paragraph::new(lines.join("\n")), frame.area());
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

#[derive(serde::Serialize, serde::Deserialize)]
struct PromptRequest {
    title: String,
    options: Vec<String>,
    input: bool,
    minimal: bool,
    #[serde(default)]
    allow_empty: bool,
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
    ensure!(
        io::stdin().is_terminal() && io::stderr().is_terminal(),
        "{title}: an interactive terminal is required"
    );
    let request = tempfile::NamedTempFile::new()?;
    let response = tempfile::NamedTempFile::new()?;
    let data = PromptRequest {
        title: title.into(),
        options: options.to_vec(),
        input,
        minimal,
        allow_empty,
    };
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
    ensure!(
        data.input || !data.options.is_empty(),
        "No options available"
    );
    let result = render_prompt(
        &data.title,
        &data.options,
        data.input,
        data.minimal,
        data.allow_empty,
    )
    .map_err(|error| error.to_string());
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

/// Resource navigation only: keys and a selection marker, sized to the list.
pub fn select_key(options: &[String]) -> Result<usize> {
    ensure!(!options.is_empty(), "No keys available");
    Ok(prompt("Resource selection", options, false, true, false)?.parse()?)
}
