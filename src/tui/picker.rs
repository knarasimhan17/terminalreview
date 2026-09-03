use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::git::{CommitLogEntry, PreparedReview};
use crate::session::ReviewSession;

use super::{ReviewOutcome, TrvTerminal, bindings, run_review, with_terminal};

pub(crate) enum ReviewTarget {
    WorkingTree,
    CommitRange {
        base_sha: String,
        source_sha: String,
    },
}

pub(crate) enum CommitPickerOutcome {
    Reviewed {
        prepared: PreparedReview,
        outcome: ReviewOutcome,
    },
    NoChanges,
    Quit,
}

enum PickerChoice {
    Review(ReviewTarget),
    NoChanges,
    Quit,
}

#[derive(Clone, Copy)]
enum PickerStep {
    Source { selected: usize },
    Base { source: usize, selected: usize },
}

struct CommitPicker {
    commits: Vec<CommitLogEntry>,
    working_tree_clean: bool,
    step: PickerStep,
    now: DateTime<Utc>,
    help: bool,
}

impl CommitPicker {
    fn new(commits: Vec<CommitLogEntry>, working_tree_clean: bool) -> Self {
        Self {
            commits,
            working_tree_clean,
            step: PickerStep::Source { selected: 0 },
            now: Utc::now(),
            help: false,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<PickerChoice> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if self.help {
            if bindings::closes_help(&key) {
                self.help = false;
            }
            return None;
        }

        let action = bindings::action_for(bindings::picker_bindings(self.context()).iter(), &key)?;
        match action {
            bindings::PickerAction::MoveDown => self.move_down(),
            bindings::PickerAction::MoveUp => self.move_up(),
            bindings::PickerAction::Select => return self.select(),
            bindings::PickerAction::Back => return self.back(),
            bindings::PickerAction::Quit => return Some(PickerChoice::Quit),
            bindings::PickerAction::Help => self.help = true,
        }
        None
    }

    fn context(&self) -> bindings::PickerContext {
        match self.step {
            PickerStep::Source { .. } => bindings::PickerContext::Source,
            PickerStep::Base { .. } => bindings::PickerContext::Base,
        }
    }

    fn move_down(&mut self) {
        let item_count = self.item_count();
        let selected = self.selected_mut();
        if *selected + 1 < item_count {
            *selected += 1;
        }
    }

    fn move_up(&mut self) {
        let selected = self.selected_mut();
        *selected = selected.saturating_sub(1);
    }

    fn select(&mut self) -> Option<PickerChoice> {
        match self.step {
            PickerStep::Source { selected: 0 } if self.working_tree_clean => {
                Some(PickerChoice::NoChanges)
            }
            PickerStep::Source { selected: 0 } => {
                Some(PickerChoice::Review(ReviewTarget::WorkingTree))
            }
            PickerStep::Source { selected } => {
                let source = selected - 1;
                let commit = self
                    .commits
                    .get(source)
                    .expect("source picker index must be bounded by the commit list");
                let selected = commit
                    .first_parent_sha
                    .as_deref()
                    .and_then(|parent| self.commits.iter().position(|commit| commit.sha == parent))
                    .unwrap_or(source);
                self.step = PickerStep::Base { source, selected };
                None
            }
            PickerStep::Base { source, selected } => {
                let source = self
                    .commits
                    .get(source)
                    .expect("base picker source index must be bounded by the commit list");
                let base = self
                    .commits
                    .get(selected)
                    .expect("base picker selection must be bounded by the commit list");
                Some(PickerChoice::Review(ReviewTarget::CommitRange {
                    base_sha: base.sha.clone(),
                    source_sha: source.sha.clone(),
                }))
            }
        }
    }

    fn back(&mut self) -> Option<PickerChoice> {
        match self.step {
            PickerStep::Source { .. } => Some(PickerChoice::Quit),
            PickerStep::Base { source, .. } => {
                self.step = PickerStep::Source {
                    selected: source + 1,
                };
                None
            }
        }
    }

    fn item_count(&self) -> usize {
        match self.step {
            PickerStep::Source { .. } => self.commits.len() + 1,
            PickerStep::Base { .. } => self.commits.len(),
        }
    }

    fn selected(&self) -> usize {
        match self.step {
            PickerStep::Source { selected } | PickerStep::Base { selected, .. } => selected,
        }
    }

    fn selected_mut(&mut self) -> &mut usize {
        match &mut self.step {
            PickerStep::Source { selected } | PickerStep::Base { selected, .. } => selected,
        }
    }
}

pub(crate) fn run(
    commits: Vec<CommitLogEntry>,
    working_tree_clean: bool,
    prepare: impl FnOnce(ReviewTarget) -> Result<PreparedReview>,
    into_session: impl FnOnce(&PreparedReview) -> Result<ReviewSession>,
) -> Result<CommitPickerOutcome> {
    with_terminal(move |terminal| {
        let mut picker = CommitPicker::new(commits, working_tree_clean);
        match choose_review_target(terminal, &mut picker)? {
            PickerChoice::Review(target) => {
                let prepared = prepare(target)?;
                let session = into_session(&prepared)?;
                let outcome = run_review(terminal, session)?;
                Ok(CommitPickerOutcome::Reviewed { prepared, outcome })
            }
            PickerChoice::NoChanges => Ok(CommitPickerOutcome::NoChanges),
            PickerChoice::Quit => Ok(CommitPickerOutcome::Quit),
        }
    })
}

fn choose_review_target(
    terminal: &mut TrvTerminal,
    picker: &mut CommitPicker,
) -> Result<PickerChoice> {
    loop {
        terminal
            .draw(|frame| render(frame, picker))
            .context("failed to draw commit picker")?;
        if let Event::Key(key) = event::read().context("failed to read terminal input")?
            && let Some(choice) = picker.handle_key(key)
        {
            return Ok(choice);
        }
    }
}

fn render(frame: &mut Frame<'_>, picker: &CommitPicker) {
    let screen = frame.area();
    let [area, footer] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(screen);
    let (title, items) = match picker.step {
        PickerStep::Source { .. } => {
            let mut items = Vec::with_capacity(picker.commits.len() + 1);
            items.push(working_tree_item(picker.working_tree_clean));
            items.extend(
                picker
                    .commits
                    .iter()
                    .map(|commit| commit_item(commit, &picker.now, area.width)),
            );
            (" trv | select commit ".to_owned(), items)
        }
        PickerStep::Base { source, .. } => {
            let source = picker
                .commits
                .get(source)
                .expect("base picker source index must be bounded by the commit list");
            let items = picker
                .commits
                .iter()
                .map(|commit| commit_item(commit, &picker.now, area.width))
                .collect();
            (
                format!(" trv | select base for {} ", source.short_sha),
                items,
            )
        }
    };
    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_symbol("> ")
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    state.select(Some(picker.selected()));
    frame.render_stateful_widget(list, area, &mut state);
    frame.render_widget(
        Paragraph::new(bindings::HELP_HINT).style(Style::default().fg(Color::DarkGray)),
        footer,
    );

    if picker.help {
        let context = picker.context();
        bindings::render_help(
            frame,
            context.help_title(),
            bindings::picker_bindings(context).iter(),
        );
    }
}

fn working_tree_item(clean: bool) -> ListItem<'static> {
    let mut spans = vec![
        Span::styled(
            "Working tree",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" (uncommitted changes vs HEAD)"),
    ];
    if clean {
        spans.push(Span::styled(
            " (no changes)",
            Style::default().fg(Color::DarkGray),
        ));
    }
    ListItem::new(Line::from(spans))
}

fn commit_item(commit: &CommitLogEntry, now: &DateTime<Utc>, area_width: u16) -> ListItem<'static> {
    // Two border columns and the two-column selection marker are unavailable to row text.
    const LIST_CHROME_WIDTH: u16 = 4;

    let prefix = format!("{}  ", commit.short_sha);
    let age = relative_age(&commit.committed_at, now);
    let indicator = if commit.unpushed { "  [unpushed]" } else { "" };
    let suffix = format!("  {age}{indicator}");
    let row_width = usize::from(area_width.saturating_sub(LIST_CHROME_WIDTH));
    let subject_width = row_width
        .saturating_sub(UnicodeWidthStr::width(prefix.as_str()))
        .saturating_sub(UnicodeWidthStr::width(suffix.as_str()));
    let subject = truncate_to_width(&commit.subject, subject_width);

    let mut spans = vec![
        Span::styled(prefix, Style::default().fg(Color::Cyan)),
        Span::raw(subject),
        Span::styled(format!("  {age}"), Style::default().fg(Color::DarkGray)),
    ];
    if commit.unpushed {
        spans.push(Span::styled(
            indicator,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    ListItem::new(Line::from(spans))
}

fn relative_age(committed_at: &DateTime<Utc>, now: &DateTime<Utc>) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    // Compact picker labels use fixed durations rather than calendar boundaries.
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;

    let seconds = now
        .timestamp()
        .saturating_sub(committed_at.timestamp())
        .max(0);
    let (value, unit) = if seconds < MINUTE {
        (seconds, "s")
    } else if seconds < HOUR {
        (seconds / MINUTE, "m")
    } else if seconds < DAY {
        (seconds / HOUR, "h")
    } else if seconds < MONTH {
        (seconds / DAY, "d")
    } else if seconds < YEAR {
        (seconds / MONTH, "mo")
    } else {
        (seconds / YEAR, "y")
    };
    format!("{value}{unit} ago")
}

fn truncate_to_width(text: &str, max_width: usize) -> String {
    const ELLIPSIS: &str = "...";

    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }
    if max_width <= ELLIPSIS.len() {
        return ".".repeat(max_width);
    }

    let target_width = max_width - ELLIPSIS.len();
    let mut truncated = String::new();
    let mut width = 0;
    for character in text.chars() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if width + character_width > target_width {
            break;
        }
        truncated.push(character);
        width += character_width;
    }
    truncated.push_str(ELLIPSIS);
    truncated
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{CommitLogEntry, CommitPicker, PickerStep, Utc};

    #[test]
    fn help_closes_without_changing_either_picker_step() {
        let commits = vec![commit("source", Some("parent")), commit("parent", None)];
        let mut picker = CommitPicker::new(commits, false);

        for close in [KeyCode::Char('?'), KeyCode::Esc, KeyCode::Char('q')] {
            picker.handle_key(key(KeyCode::Char('?')));
            assert!(picker.help, "? must open help in the source picker");
            picker.handle_key(key(KeyCode::Char('j')));
            picker.handle_key(key(close));
            assert!(!picker.help, "a help close key must dismiss the overlay");
            assert!(
                matches!(picker.step, PickerStep::Source { selected: 0 }),
                "help must preserve the source picker selection"
            );
        }

        picker.handle_key(key(KeyCode::Down));
        picker.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(
                picker.step,
                PickerStep::Base {
                    source: 0,
                    selected: 1
                }
            ),
            "selecting a commit must open its parent in the base picker"
        );

        picker.handle_key(key(KeyCode::Char('?')));
        picker.handle_key(key(KeyCode::Up));
        picker.handle_key(key(KeyCode::Char('q')));
        assert!(
            matches!(
                picker.step,
                PickerStep::Base {
                    source: 0,
                    selected: 1
                }
            ),
            "closing help must preserve the base picker selection"
        );
    }

    fn commit(sha: &str, first_parent_sha: Option<&str>) -> CommitLogEntry {
        CommitLogEntry {
            sha: sha.to_owned(),
            short_sha: sha.to_owned(),
            first_parent_sha: first_parent_sha.map(str::to_owned),
            subject: sha.to_owned(),
            committed_at: Utc::now(),
            unpushed: false,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
}
