mod diff_view;
mod picker;

use std::io;

use anyhow::{Context, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use unicode_width::UnicodeWidthStr;

use crate::diff::{DiffLine, LineAnchor, ParsedDiff};
use crate::model::Comment;

pub(crate) use picker::{CommitPickerOutcome, ReviewTarget, run as run_picker};

type TrvTerminal = Terminal<CrosstermBackend<io::Stdout>>;

pub(crate) enum ReviewOutcome {
    Export(Vec<Comment>),
    Quit,
}

#[derive(Clone, Copy)]
enum View {
    Diff,
    Comments,
}

enum Mode {
    Diff,
    Comments,
    CommentInput { anchor: LineAnchor, body: String },
    QuitConfirm { previous: View },
}

struct App {
    diff: ParsedDiff,
    comments: Vec<Comment>,
    selected_diff: usize,
    selected_comment: usize,
    inline_comments: bool,
    mode: Mode,
    status: Option<String>,
}

impl App {
    fn new(diff: ParsedDiff) -> Self {
        Self {
            diff,
            comments: Vec::new(),
            selected_diff: 0,
            selected_comment: 0,
            inline_comments: true,
            mode: Mode::Diff,
            status: None,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<ReviewOutcome> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return self.request_quit();
        }

        if matches!(self.mode, Mode::Diff) {
            self.handle_diff_key(key)
        } else if matches!(self.mode, Mode::Comments) {
            self.handle_comments_key(key)
        } else if matches!(self.mode, Mode::CommentInput { .. }) {
            self.handle_input_key(key)
        } else {
            self.handle_quit_key(key)
        }
    }

    fn handle_diff_key(&mut self, key: KeyEvent) -> Option<ReviewOutcome> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_diff_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_diff_up(),
            KeyCode::Char('g') | KeyCode::Home => self.select_diff(0),
            KeyCode::Char('G') | KeyCode::End => {
                self.select_diff(self.diff.lines.len().saturating_sub(1));
            }
            KeyCode::Char(']') => self.next_file(),
            KeyCode::Char('[') => self.previous_file(),
            KeyCode::Char('c') => self.start_comment(),
            KeyCode::Char('v') => {
                self.inline_comments = !self.inline_comments;
                let state = if self.inline_comments {
                    "shown"
                } else {
                    "hidden"
                };
                self.status = Some(format!("Inline comments {state}."));
            }
            KeyCode::Char('l') | KeyCode::Tab => self.mode = Mode::Comments,
            KeyCode::Char('y') => {
                return Some(ReviewOutcome::Export(self.comments.clone()));
            }
            KeyCode::Char('q') | KeyCode::Esc => return self.request_quit(),
            _ => {}
        }
        None
    }

    fn handle_comments_key(&mut self, key: KeyEvent) -> Option<ReviewOutcome> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down
                if self.selected_comment + 1 < self.comments.len() =>
            {
                self.selected_comment += 1;
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected_comment = self.selected_comment.saturating_sub(1);
            }
            KeyCode::Char('g') | KeyCode::Home => self.selected_comment = 0,
            KeyCode::Char('G') | KeyCode::End => {
                self.selected_comment = self.comments.len().saturating_sub(1);
            }
            KeyCode::Char('l') | KeyCode::Tab | KeyCode::Esc => self.mode = Mode::Diff,
            KeyCode::Char('y') => {
                return Some(ReviewOutcome::Export(self.comments.clone()));
            }
            KeyCode::Char('q') => return self.request_quit(),
            _ => {}
        }
        None
    }

    fn handle_input_key(&mut self, key: KeyEvent) -> Option<ReviewOutcome> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Diff;
                self.status = Some("Comment canceled.".to_owned());
            }
            KeyCode::Backspace => {
                if let Mode::CommentInput { body, .. } = &mut self.mode {
                    body.pop();
                }
            }
            KeyCode::Enter => {
                let mode = std::mem::replace(&mut self.mode, Mode::Diff);
                let Mode::CommentInput { anchor, body } = mode else {
                    unreachable!("input handling requires comment-input mode");
                };
                let body = body.trim().to_owned();
                if body.is_empty() {
                    self.status = Some("Empty comment ignored.".to_owned());
                } else {
                    self.comments
                        .push(Comment::open(anchor.path, anchor.line, anchor.side, body));
                    self.selected_comment = self.comments.len().saturating_sub(1);
                    self.status = Some("Comment added.".to_owned());
                }
            }
            KeyCode::Char(character)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                if let Mode::CommentInput { body, .. } = &mut self.mode {
                    body.push(character);
                }
            }
            _ => {}
        }
        None
    }

    fn handle_quit_key(&mut self, key: KeyEvent) -> Option<ReviewOutcome> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(ReviewOutcome::Quit),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') | KeyCode::Esc => {
                let mode = std::mem::replace(&mut self.mode, Mode::Diff);
                let Mode::QuitConfirm { previous } = mode else {
                    unreachable!("quit handling requires quit-confirm mode");
                };
                self.mode = match previous {
                    View::Diff => Mode::Diff,
                    View::Comments => Mode::Comments,
                };
                None
            }
            _ => None,
        }
    }

    fn move_diff_down(&mut self) {
        if self.selected_diff + 1 < self.diff.lines.len() {
            self.select_diff(self.selected_diff + 1);
        }
    }

    fn move_diff_up(&mut self) {
        self.select_diff(self.selected_diff.saturating_sub(1));
    }

    fn select_diff(&mut self, index: usize) {
        self.selected_diff = index.min(self.diff.lines.len().saturating_sub(1));
        self.status = None;
    }

    fn next_file(&mut self) {
        if let Some(index) = self
            .diff
            .file_starts
            .iter()
            .copied()
            .find(|index| *index > self.selected_diff)
        {
            self.select_diff(index);
        }
    }

    fn previous_file(&mut self) {
        if let Some(index) = self
            .diff
            .file_starts
            .iter()
            .copied()
            .rev()
            .find(|index| *index < self.selected_diff)
        {
            self.select_diff(index);
        }
    }

    fn start_comment(&mut self) {
        let Some(anchor) = self.diff.lines[self.selected_diff].anchor().cloned() else {
            self.status = Some("Select a changed or context line to comment.".to_owned());
            return;
        };
        self.mode = Mode::CommentInput {
            anchor,
            body: String::new(),
        };
        self.status = None;
    }

    fn request_quit(&mut self) -> Option<ReviewOutcome> {
        if self.comments.is_empty() {
            return Some(ReviewOutcome::Quit);
        }
        let previous = match self.mode {
            Mode::Comments => View::Comments,
            Mode::Diff | Mode::CommentInput { .. } | Mode::QuitConfirm { .. } => View::Diff,
        };
        self.mode = Mode::QuitConfirm { previous };
        None
    }

    fn visible_view(&self) -> View {
        match self.mode {
            Mode::Diff | Mode::CommentInput { .. } => View::Diff,
            Mode::Comments => View::Comments,
            Mode::QuitConfirm { previous } => previous,
        }
    }

    fn selected_anchor(&self) -> Option<&LineAnchor> {
        self.diff.lines[self.selected_diff].anchor()
    }

    fn comments_for_line<'a>(
        &'a self,
        line: &'a DiffLine,
    ) -> impl Iterator<Item = &'a Comment> + 'a {
        let anchor = line.anchor();
        self.comments.iter().filter(move |comment| {
            anchor.is_some_and(|anchor| {
                comment.path == anchor.path
                    && comment.line == anchor.line
                    && comment.side == anchor.side
            })
        })
    }
}

pub(crate) fn run(diff: ParsedDiff) -> Result<ReviewOutcome> {
    with_terminal(|terminal| run_review(terminal, diff))
}

fn with_terminal<T>(operation: impl FnOnce(&mut TrvTerminal) -> Result<T>) -> Result<T> {
    let _terminal_mode = TerminalMode::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to initialize terminal")?;
    terminal.clear().context("failed to clear terminal")?;
    operation(&mut terminal)
}

fn run_review(terminal: &mut TrvTerminal, diff: ParsedDiff) -> Result<ReviewOutcome> {
    let mut app = App::new(diff);

    loop {
        terminal
            .draw(|frame| render(frame, &app))
            .context("failed to draw review")?;
        if let Event::Key(key) = event::read().context("failed to read terminal input")?
            && let Some(outcome) = app.handle_key(key)
        {
            return Ok(outcome);
        }
    }
}

struct TerminalMode {
    raw: bool,
    alternate: bool,
}

impl TerminalMode {
    fn enter() -> Result<Self> {
        let mut mode = Self {
            raw: false,
            alternate: false,
        };
        enable_raw_mode().context("failed to enable terminal raw mode")?;
        mode.raw = true;

        let mut output = io::stdout();
        execute!(output, EnterAlternateScreen).context("failed to enter alternate screen")?;
        mode.alternate = true;
        execute!(output, Hide).context("failed to hide terminal cursor")?;
        Ok(mode)
    }
}

impl Drop for TerminalMode {
    fn drop(&mut self) {
        let mut output = io::stdout();
        let screen_result = if self.alternate {
            execute!(output, Show, LeaveAlternateScreen)
        } else {
            execute!(output, Show)
        };
        if let Err(error) = screen_result {
            eprintln!("trv: failed to restore terminal screen: {error}");
        }
        if self.raw
            && let Err(error) = disable_raw_mode()
        {
            eprintln!("trv: failed to disable terminal raw mode: {error}");
        }
    }
}

fn render(frame: &mut Frame<'_>, app: &App) {
    let [content, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());

    match app.visible_view() {
        View::Diff => diff_view::render(frame, content, app),
        View::Comments => render_comments(frame, content, app),
    }
    render_footer(frame, footer, app);

    match &app.mode {
        Mode::CommentInput { body, .. } => render_comment_input(frame, body),
        Mode::QuitConfirm { .. } => render_quit_confirm(frame, app.comments.len()),
        Mode::Diff | Mode::Comments => {}
    }
}

fn render_comments(frame: &mut Frame<'_>, area: Rect, app: &App) {
    let items = if app.comments.is_empty() {
        vec![ListItem::new("No comments.")]
    } else {
        app.comments
            .iter()
            .map(|comment| {
                ListItem::new(format!(
                    "{}:{} [{}] {}",
                    comment.path,
                    comment.line,
                    comment.side.as_str(),
                    comment.body
                ))
            })
            .collect()
    };
    let list = List::new(items)
        .block(Block::bordered().title(format!(" trv | comments | {} ", app.comments.len())))
        .highlight_symbol("> ")
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    if !app.comments.is_empty() {
        state.select(Some(app.selected_comment));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &App) {
    frame.render_widget(
        Paragraph::new(footer_text(app)).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn footer_text(app: &App) -> String {
    let detail = app.status.clone().unwrap_or_else(|| {
        app.selected_anchor()
            .map(|anchor| format!("{}:{} [{}]", anchor.path, anchor.line, anchor.side.as_str()))
            .unwrap_or_default()
    });
    let inline_state = if app.inline_comments { "on" } else { "off" };
    if detail.is_empty() {
        format!("v inline comments: {inline_state}")
    } else {
        format!("v inline comments: {inline_state} | {detail}")
    }
}

fn render_comment_input(frame: &mut Frame<'_>, body: &str) {
    // 72 columns keeps a long review comment visible without hiding the surrounding diff.
    const MAX_WIDTH: u16 = 72;

    let screen = frame.area();
    let width = screen.width.saturating_sub(4).min(MAX_WIDTH);
    let area = centered(screen, width, 3);
    frame.render_widget(Clear, area);

    let visible_width = usize::from(area.width.saturating_sub(2));
    let body_width = UnicodeWidthStr::width(body);
    let scroll = body_width.saturating_sub(visible_width.saturating_sub(1));
    let horizontal_scroll = scroll.min(usize::from(u16::MAX)) as u16;
    let input = Paragraph::new(body)
        .block(Block::bordered().title(" Comment "))
        .scroll((0, horizontal_scroll));
    frame.render_widget(input, area);

    if area.width > 2 && area.height > 2 {
        let cursor_offset = body_width
            .saturating_sub(scroll)
            .min(visible_width.saturating_sub(1));
        frame.set_cursor_position(Position::new(area.x + 1 + cursor_offset as u16, area.y + 1));
    }
}

fn render_quit_confirm(frame: &mut Frame<'_>, comment_count: usize) {
    let suffix = if comment_count == 1 { "" } else { "s" };
    let prompt = format!("Discard {comment_count} unexported comment{suffix}? (y/N)");
    let screen = frame.area();
    let desired_width = UnicodeWidthStr::width(prompt.as_str()).saturating_add(4);
    let width = desired_width.min(usize::from(screen.width)) as u16;
    let area = centered(screen, width, 3);
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(prompt)
            .alignment(Alignment::Center)
            .block(Block::bordered().title(" Quit ")),
        area,
    );
}

fn centered(outer: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(outer.width);
    let height = height.min(outer.height);
    Rect::new(
        outer.x + outer.width.saturating_sub(width) / 2,
        outer.y + outer.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::{App, KeyCode, KeyEvent, KeyModifiers, ParsedDiff, footer_text};

    #[test]
    fn inline_comment_toggle_preserves_the_selected_diff_line() {
        let mut app = App::new(ParsedDiff::parse("first\nsecond"));
        app.selected_diff = 1;
        let selected = app.selected_diff;

        assert!(app.inline_comments, "inline comments must start visible");
        assert!(
            footer_text(&app).contains("v inline comments: on"),
            "the footer must document the toggle and its state"
        );

        app.handle_diff_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

        assert!(!app.inline_comments, "v must hide inline comment bodies");
        assert_eq!(
            app.selected_diff, selected,
            "toggling inline comments must preserve the selected diff line"
        );
    }
}
