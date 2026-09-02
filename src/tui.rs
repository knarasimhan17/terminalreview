mod bindings;
mod comment_input;
mod diff_view;
mod picker;
mod review;

use std::io;

use anyhow::{Context, Result};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{Frame, Terminal};
use unicode_width::UnicodeWidthStr;

use crate::diff::{LineAnchor, ParsedDiff, SideBySideRow};
use crate::model::{Comment, Side};

pub(crate) use picker::{CommitPickerOutcome, ReviewTarget, run as run_picker};

type TrvTerminal = Terminal<CrosstermBackend<io::Stdout>>;

pub(crate) enum ReviewOutcome {
    Export(Vec<Comment>),
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffLayout {
    Unified,
    SideBySide,
}

impl DiffLayout {
    fn as_str(self) -> &'static str {
        match self {
            Self::Unified => "unified",
            Self::SideBySide => "side-by-side",
        }
    }
}

struct App {
    diff: ParsedDiff,
    collapsed_files: Vec<bool>,
    comments: Vec<Comment>,
    selected_diff: usize,
    selected_side: Side,
    selected_comment: usize,
    diff_layout: DiffLayout,
    inline_comments: bool,
    mode: Mode,
    status: Option<String>,
    help: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffRow {
    File(usize),
    Line { file: usize, line: usize },
    SideBySide { file: usize, row: SideBySideRow },
}

impl App {
    fn new(diff: ParsedDiff) -> Self {
        let collapsed_files = vec![false; diff.files.len()];
        Self {
            diff,
            collapsed_files,
            comments: Vec::new(),
            selected_diff: 0,
            selected_side: Side::New,
            selected_comment: 0,
            diff_layout: DiffLayout::Unified,
            inline_comments: true,
            mode: Mode::Diff,
            status: None,
            help: false,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<ReviewOutcome> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if self.help {
            if bindings::closes_help(&key) {
                self.help = false;
            }
            return None;
        }

        if matches!(self.mode, Mode::Diff) {
            self.handle_diff_key(key)
        } else if matches!(self.mode, Mode::Comments) {
            self.handle_comments_key(key)
        } else if matches!(self.mode, Mode::CommentInput { .. }) {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                self.request_quit()
            } else {
                self.handle_input_key(key)
            }
        } else if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.request_quit()
        } else {
            self.handle_quit_key(key)
        }
    }

    fn handle_diff_key(&mut self, key: KeyEvent) -> Option<ReviewOutcome> {
        let context = self.review_context();
        let action = bindings::action_for(bindings::review_bindings(context), &key)?;
        match action {
            bindings::ReviewAction::MoveDown => self.move_diff_down(),
            bindings::ReviewAction::MoveUp => self.move_diff_up(),
            bindings::ReviewAction::First => self.select_diff(0),
            bindings::ReviewAction::Last => {
                self.select_diff(self.diff_rows().len().saturating_sub(1));
            }
            bindings::ReviewAction::NextFile => self.next_file(),
            bindings::ReviewAction::PreviousFile => self.previous_file(),
            bindings::ReviewAction::SelectOld => self.select_side(Side::Old),
            bindings::ReviewAction::SelectNew => self.select_side(Side::New),
            bindings::ReviewAction::ToggleFile => {
                self.toggle_selected_file();
            }
            bindings::ReviewAction::ToggleFileOrComments => {
                if !self.toggle_selected_file() {
                    self.mode = Mode::Comments;
                }
            }
            bindings::ReviewAction::AddComment => self.start_comment(),
            bindings::ReviewAction::OpenComments => self.mode = Mode::Comments,
            bindings::ReviewAction::ToggleLayout => self.toggle_diff_layout(),
            bindings::ReviewAction::ToggleInlineComments => {
                self.inline_comments = !self.inline_comments;
                let state = if self.inline_comments {
                    "shown"
                } else {
                    "hidden"
                };
                self.status = Some(format!("Inline comments {state}."));
            }
            bindings::ReviewAction::Export => {
                return Some(ReviewOutcome::Export(self.comments.clone()));
            }
            bindings::ReviewAction::Quit => return self.request_quit(),
            bindings::ReviewAction::Help => self.help = true,
            bindings::ReviewAction::ReturnToDiff => {
                unreachable!("diff keymap cannot contain comment-list actions")
            }
        }
        None
    }

    fn handle_comments_key(&mut self, key: KeyEvent) -> Option<ReviewOutcome> {
        let action = bindings::action_for(
            bindings::review_bindings(bindings::ReviewContext::Comments),
            &key,
        )?;
        match action {
            bindings::ReviewAction::MoveDown => {
                if self.selected_comment + 1 < self.comments.len() {
                    self.selected_comment += 1;
                }
            }
            bindings::ReviewAction::MoveUp => {
                self.selected_comment = self.selected_comment.saturating_sub(1);
            }
            bindings::ReviewAction::First => self.selected_comment = 0,
            bindings::ReviewAction::Last => {
                self.selected_comment = self.comments.len().saturating_sub(1);
            }
            bindings::ReviewAction::ReturnToDiff => self.mode = Mode::Diff,
            bindings::ReviewAction::Export => {
                return Some(ReviewOutcome::Export(self.comments.clone()));
            }
            bindings::ReviewAction::Quit => return self.request_quit(),
            bindings::ReviewAction::Help => self.help = true,
            bindings::ReviewAction::NextFile
            | bindings::ReviewAction::PreviousFile
            | bindings::ReviewAction::SelectOld
            | bindings::ReviewAction::SelectNew
            | bindings::ReviewAction::ToggleFile
            | bindings::ReviewAction::ToggleFileOrComments
            | bindings::ReviewAction::AddComment
            | bindings::ReviewAction::OpenComments
            | bindings::ReviewAction::ToggleInlineComments
            | bindings::ReviewAction::ToggleLayout => {
                unreachable!("comment-list keymap cannot contain diff actions")
            }
        }
        None
    }

    fn review_context(&self) -> bindings::ReviewContext {
        match self.visible_view() {
            View::Comments => bindings::ReviewContext::Comments,
            View::Diff if self.diff_layout == DiffLayout::Unified => {
                bindings::ReviewContext::Unified
            }
            View::Diff => bindings::ReviewContext::SideBySide,
        }
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

    let selected_row = match app.visible_view() {
        View::Diff => diff_view::render(frame, content, app),
        View::Comments => {
            render_comments(frame, content, app);
            None
        }
    };
    render_footer(frame, footer, app);

    match &app.mode {
        Mode::CommentInput { body, .. } => {
            comment_input::render(frame, content, body, selected_row)
        }
        Mode::QuitConfirm { .. } => render_quit_confirm(frame, app.comments.len()),
        Mode::Diff | Mode::Comments => {}
    }
    if app.help {
        let context = app.review_context();
        bindings::render_help(
            frame,
            context.help_title(),
            bindings::review_bindings(context),
        );
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
        if app.visible_view() == View::Diff {
            app.selected_anchor()
                .map(|anchor| format!("{}:{} [{}]", anchor.path, anchor.line, anchor.side.as_str()))
                .unwrap_or_default()
        } else {
            String::new()
        }
    });
    let controls = match app.visible_view() {
        View::Diff => {
            let inline_state = if app.inline_comments { "on" } else { "off" };
            format!(
                "s view: {} | v inline comments: {inline_state} | {}",
                app.diff_layout.as_str(),
                bindings::HELP_HINT
            )
        }
        View::Comments => format!(
            "l/Tab/Esc review | y export | q quit | {}",
            bindings::HELP_HINT
        ),
    };
    if detail.is_empty() {
        controls
    } else {
        format!("{controls} | {detail}")
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

pub(super) fn centered(outer: Rect, width: u16, height: u16) -> Rect {
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
mod tests;
