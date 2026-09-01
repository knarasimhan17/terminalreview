use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::diff::{DiffLine, DiffLineKind};

use super::App;

pub(super) fn render(frame: &mut Frame<'_>, area: Rect, app: &App) {
    const HIGHLIGHT_SYMBOL: &str = "> ";

    let block = Block::bordered().title(format!(" trv | diff | {} comments ", app.comments.len()));
    let line_width = usize::from(block.inner(area).width)
        .saturating_sub(UnicodeWidthStr::width(HIGHLIGHT_SYMBOL));
    let items = app
        .diff
        .lines
        .iter()
        .map(|line| ListItem::new(item_lines(app, line, line_width)))
        .collect::<Vec<_>>();
    let list = List::new(items)
        .block(block)
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
    let mut state = ListState::default();
    state.select(Some(app.selected_diff));
    frame.render_stateful_widget(list, area, &mut state);
}

fn item_lines(app: &App, line: &DiffLine, width: usize) -> Vec<Line<'static>> {
    const COMMENT_PREFIX: &str = "┃ ";

    let comments = app.comments_for_line(line).collect::<Vec<_>>();
    let marker = if comments.is_empty() { " " } else { "●" };
    let old_line = line
        .old_line
        .map(|line| line.to_string())
        .unwrap_or_default();
    let new_line = line
        .new_line
        .map(|line| line.to_string())
        .unwrap_or_default();
    let gutter = format!("{marker} {old_line:>5} {new_line:>5} ");
    let mut rows = vec![Line::from(vec![
        Span::styled(gutter, Style::default().fg(Color::DarkGray)),
        Span::styled(line.text.clone(), line_style(line.kind)),
    ])];

    if app.inline_comments {
        let body_width = width
            .saturating_sub(UnicodeWidthStr::width(COMMENT_PREFIX))
            .max(1);
        let comment_style = Style::default().add_modifier(Modifier::DIM | Modifier::REVERSED);
        for comment in comments {
            rows.extend(
                wrap_comment_body(&comment.body, body_width)
                    .into_iter()
                    .map(|body| Line::styled(format!("{COMMENT_PREFIX}{body}"), comment_style)),
            );
        }
    }

    rows
}

fn wrap_comment_body(body: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();

    for source_line in body.split('\n') {
        let mut remaining = source_line;
        if remaining.is_empty() {
            rows.push(String::new());
            continue;
        }

        while UnicodeWidthStr::width(remaining) > width {
            let hard_break = byte_index_at_width(remaining, width);
            let soft_break = remaining[..hard_break]
                .rfind(|character: char| character.is_whitespace())
                .filter(|index| *index > 0);
            let split = soft_break.unwrap_or(hard_break);
            rows.push(remaining[..split].trim_end().to_owned());
            remaining = remaining[split..].trim_start();
        }

        if !remaining.is_empty() {
            rows.push(remaining.to_owned());
        }
    }

    rows
}

fn byte_index_at_width(text: &str, width: usize) -> usize {
    let mut used = 0;
    for (index, character) in text.char_indices() {
        let character_width = UnicodeWidthChar::width(character).unwrap_or(0);
        if used + character_width > width {
            return if index == 0 {
                character.len_utf8()
            } else {
                index
            };
        }
        used += character_width;
    }
    text.len()
}

fn line_style(kind: DiffLineKind) -> Style {
    match kind {
        DiffLineKind::File => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        DiffLineKind::Hunk => Style::default().fg(Color::Cyan),
        DiffLineKind::Addition => Style::default().fg(Color::Green),
        DiffLineKind::Deletion => Style::default().fg(Color::Red),
        DiffLineKind::Context => Style::default(),
        DiffLineKind::Meta => Style::default().fg(Color::DarkGray),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::diff::{DiffLineKind, ParsedDiff};

    use super::super::Mode;
    use super::{App, item_lines, wrap_comment_body};

    // Ten body columns force both word-boundary and hard-word wrapping.
    const NARROW_WIDTH: usize = 12;
    const DIFF: &str = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1 +1 @@
-old
+new
";

    #[test]
    fn inline_comments_follow_their_diff_line_and_wrap() {
        let mut app = App::new(ParsedDiff::parse(DIFF));
        let selected = app
            .diff
            .lines
            .iter()
            .position(|line| line.kind == DiffLineKind::Addition)
            .expect("the fixture must contain an added line");
        app.selected_diff = selected;
        app.start_comment();
        let Mode::CommentInput { body, .. } = &mut app.mode else {
            panic!("commenting on an added line must open the input");
        };
        body.push_str("first exceptionallylong\n\nnext");
        app.handle_input_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let visible = item_lines(&app, &app.diff.lines[selected], NARROW_WIDTH)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            visible,
            [
                "●           1 +new",
                "┃ first",
                "┃ exceptiona",
                "┃ llylong",
                "┃ ",
                "┃ next",
            ],
            "inline rows must follow the anchored diff line and fit the available width"
        );
        assert_eq!(
            wrap_comment_body("界", 1),
            ["界"],
            "wrapping a wide leading character must still make progress"
        );

        app.inline_comments = false;
        let hidden = item_lines(&app, &app.diff.lines[selected], NARROW_WIDTH)
            .into_iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            hidden,
            ["●           1 +new"],
            "hiding inline bodies must retain the gutter marker"
        );
    }
}
