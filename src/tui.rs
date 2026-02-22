use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusField {
    Input,
    Output,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusKind {
    Info,
    Success,
    Error,
}

#[derive(Debug)]
struct AppState {
    input: String,
    output: String,
    focus: FocusField,
    status: String,
    status_kind: StatusKind,
    should_quit: bool,
}

impl AppState {
    fn new(input: Option<PathBuf>, output: Option<PathBuf>) -> Self {
        let input_str = input
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let output_str = output
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .or_else(|| {
                if input_str.is_empty() {
                    None
                } else {
                    Some(derive_output_path(&input_str))
                }
            })
            .unwrap_or_default();

        Self {
            input: input_str,
            output: output_str,
            focus: FocusField::Input,
            status: "Ready. Tab: switch field | Enter: convert | q/Esc: quit".to_string(),
            status_kind: StatusKind::Info,
            should_quit: false,
        }
    }

    fn focused_mut(&mut self) -> &mut String {
        match self.focus {
            FocusField::Input => &mut self.input,
            FocusField::Output => &mut self.output,
        }
    }

    fn switch_focus(&mut self) {
        self.focus = match self.focus {
            FocusField::Input => FocusField::Output,
            FocusField::Output => FocusField::Input,
        };
    }

    fn append_str_to_focus(&mut self, s: &str) {
        self.focused_mut().push_str(s);
    }

    fn backspace(&mut self) {
        self.focused_mut().pop();
    }

    fn clear_focused(&mut self) {
        self.focused_mut().clear();
    }
}

/// Run interactive TUI conversion workflow.
pub fn run_tui<F>(input: Option<PathBuf>, output: Option<PathBuf>, convert: F) -> anyhow::Result<()>
where
    F: Fn(&Path, &Path) -> anyhow::Result<()>,
{
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = AppState::new(input, output);
    let result = tui_loop(&mut terminal, &mut app, convert);

    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;

    result
}

fn tui_loop<F>(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut AppState,
    convert: F,
) -> anyhow::Result<()>
where
    F: Fn(&Path, &Path) -> anyhow::Result<()>,
{
    loop {
        terminal.draw(|frame| draw(frame, app))?;
        if app.should_quit {
            break;
        }

        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    handle_key(app, key, &convert);
                }
                Event::Paste(text) => app.append_str_to_focus(&text),
                _ => {}
            }
        }
    }
    Ok(())
}

fn handle_key<F>(app: &mut AppState, key: KeyEvent, convert: &F)
where
    F: Fn(&Path, &Path) -> anyhow::Result<()>,
{
    match key.code {
        KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('q') if key.modifiers.is_empty() => app.should_quit = true,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        KeyCode::Tab | KeyCode::Up | KeyCode::Down => app.switch_focus(),
        KeyCode::Backspace => app.backspace(),
        KeyCode::Enter => run_conversion(app, convert),
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => app.clear_focused(),
        KeyCode::Char(ch) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            app.focused_mut().push(ch);
        }
        _ => {}
    }
}

fn run_conversion<F>(app: &mut AppState, convert: &F)
where
    F: Fn(&Path, &Path) -> anyhow::Result<()>,
{
    let input = app.input.trim();
    if input.is_empty() {
        app.status = "Input is required.".to_string();
        app.status_kind = StatusKind::Error;
        return;
    }

    if app.output.trim().is_empty() {
        app.output = derive_output_path(input);
    }

    let output = app.output.trim();
    if output.is_empty() {
        app.status = "Output is required.".to_string();
        app.status_kind = StatusKind::Error;
        return;
    }

    match convert(Path::new(input), Path::new(output)) {
        Ok(()) => {
            app.status = format!("Conversion completed: {}", output);
            app.status_kind = StatusKind::Success;
        }
        Err(err) => {
            app.status = format!("Conversion failed: {err}");
            app.status_kind = StatusKind::Error;
        }
    }
}

fn draw(frame: &mut Frame<'_>, app: &AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(frame.area());

    let title = Paragraph::new("ferritex TUI")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL).title("Mode"));
    frame.render_widget(title, layout[0]);

    let input_style = if app.focus == FocusField::Input {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let output_style = if app.focus == FocusField::Output {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let input = Paragraph::new(app.input.as_str())
        .style(input_style)
        .block(Block::default().borders(Borders::ALL).title("Input .tex"));
    frame.render_widget(input, layout[1]);

    let output = Paragraph::new(app.output.as_str())
        .style(output_style)
        .block(Block::default().borders(Borders::ALL).title("Output .docx"));
    frame.render_widget(output, layout[2]);

    let status_color = match app.status_kind {
        StatusKind::Info => Color::White,
        StatusKind::Success => Color::Green,
        StatusKind::Error => Color::Red,
    };
    let status = Paragraph::new(app.status.as_str())
        .style(Style::default().fg(status_color))
        .block(Block::default().borders(Borders::ALL).title("Status"));
    frame.render_widget(status, layout[3]);

    let help = Paragraph::new(vec![Line::from(vec![
        Span::raw("Tab"),
        Span::raw(" switch | "),
        Span::raw("Enter"),
        Span::raw(" convert | "),
        Span::raw("Ctrl+U"),
        Span::raw(" clear field | "),
        Span::raw("q/Esc"),
        Span::raw(" quit"),
    ])])
    .block(Block::default().borders(Borders::ALL).title("Keys"));
    frame.render_widget(help, layout[4]);

    match app.focus {
        FocusField::Input => set_cursor(frame, layout[1], &app.input),
        FocusField::Output => set_cursor(frame, layout[2], &app.output),
    }
}

fn set_cursor(frame: &mut Frame<'_>, area: Rect, text: &str) {
    let inner_width = area.width.saturating_sub(2);
    let text_len_u16 = match u16::try_from(text.chars().count()) {
        Ok(v) => v,
        Err(_) => u16::MAX,
    };
    let cursor_x = area
        .x
        .saturating_add(1)
        .saturating_add(text_len_u16.min(inner_width));
    let cursor_y = area.y.saturating_add(1);
    frame.set_cursor_position((cursor_x, cursor_y));
}

fn derive_output_path(input: &str) -> String {
    let path = Path::new(input);
    if let Some(parent) = path.parent()
        && let Some(stem) = path.file_stem()
    {
        let mut out = PathBuf::new();
        if !parent.as_os_str().is_empty() {
            out.push(parent);
        }
        out.push(stem);
        out.set_extension("docx");
        return out.to_string_lossy().to_string();
    }

    if let Some(stem) = path.file_stem() {
        let mut out = PathBuf::from(stem);
        out.set_extension("docx");
        return out.to_string_lossy().to_string();
    }

    format!("{input}.docx")
}

#[cfg(test)]
mod tests {
    use super::derive_output_path;

    #[test]
    fn test_derive_output_path_from_tex() {
        assert_eq!(derive_output_path("main.tex"), "main.docx");
    }

    #[test]
    fn test_derive_output_path_with_parent() {
        assert_eq!(derive_output_path("docs/main.tex"), "docs/main.docx");
    }

    #[test]
    fn test_derive_output_path_without_extension() {
        assert_eq!(derive_output_path("main"), "main.docx");
    }
}
