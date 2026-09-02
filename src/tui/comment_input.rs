use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::centered;
use super::diff_view::wrap_comment_body;

// 72 columns keeps a long review comment visible without hiding the surrounding diff.
const MAX_WIDTH: u16 = 72;
const MAX_INNER_HEIGHT: u16 = 8;

struct CommentInputLayout {
    area: Rect,
    text: String,
    scroll: u16,
    cursor: Position,
}

pub(super) fn render(frame: &mut Frame<'_>, body: &str) {
    let layout = layout_comment_input(frame.area(), body);
    frame.render_widget(Clear, layout.area);
    frame.render_widget(
        Paragraph::new(layout.text)
            .block(Block::bordered().title(" Comment "))
            .scroll((layout.scroll, 0)),
        layout.area,
    );
    if layout.area.width > 2 && layout.area.height > 2 {
        frame.set_cursor_position(layout.cursor);
    }
}

fn layout_comment_input(screen: Rect, body: &str) -> CommentInputLayout {
    let width = screen.width.saturating_sub(4).min(MAX_WIDTH);
    let width = width.max(3.min(screen.width));
    let inner_width = usize::from(width.saturating_sub(2)).max(1);
    let mut lines = wrap_comment_body(body, inner_width);
    if lines.is_empty() {
        lines.push(String::new());
    }
    if UnicodeWidthStr::width(lines.last().map(String::as_str).unwrap_or("")) >= inner_width {
        lines.push(String::new());
    }

    let max_inner = screen.height.saturating_sub(2).min(MAX_INNER_HEIGHT).max(1);
    let visible_inner = (lines.len() as u16).clamp(1, max_inner);
    let height = visible_inner.saturating_add(2).min(screen.height);
    let area = centered(screen, width, height);
    let inner_height = area.height.saturating_sub(2).max(1);
    let scroll = (lines.len() as u16).saturating_sub(inner_height);
    let cursor_col = UnicodeWidthStr::width(lines.last().map(String::as_str).unwrap_or("")) as u16;
    let cursor = Position::new(
        area.x.saturating_add(1).saturating_add(cursor_col),
        area.y.saturating_add(1).saturating_add(
            (lines.len() as u16)
                .saturating_sub(1)
                .saturating_sub(scroll),
        ),
    );

    CommentInputLayout {
        area,
        text: lines.join("\n"),
        scroll,
        cursor,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use unicode_width::UnicodeWidthStr;

    use super::{MAX_INNER_HEIGHT, MAX_WIDTH, layout_comment_input, render};

    fn inner_width(area: Rect) -> usize {
        usize::from(area.width.saturating_sub(2)).max(1)
    }

    #[test]
    fn short_comment_stays_one_line_tall() {
        let layout = layout_comment_input(Rect::new(0, 0, 80, 24), "looks good");
        assert_eq!(
            layout.area.height, 3,
            "a short comment must keep the original single-line popup height"
        );
        assert_eq!(layout.text, "looks good");
        assert_eq!(layout.scroll, 0);
    }

    #[test]
    fn long_comment_wraps_and_grows_instead_of_scrolling_sideways() {
        let body = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda";
        let layout = layout_comment_input(Rect::new(0, 0, 40, 24), body);
        let wrapped: Vec<&str> = layout.text.lines().collect();

        assert!(
            layout.area.height > 3,
            "wrapping a long comment must grow the popup: {:?}",
            layout.area
        );
        assert!(
            wrapped.len() > 1,
            "a long comment must occupy more than one visual row: {wrapped:?}"
        );
        assert!(
            wrapped
                .iter()
                .all(|line| UnicodeWidthStr::width(*line) <= inner_width(layout.area)),
            "no wrapped row may exceed the popup inner width: {wrapped:?}"
        );
        assert!(
            wrapped.iter().any(|line| line.contains("alpha"))
                && wrapped.iter().any(|line| line.contains("lambda")),
            "wrapping must keep the start and end of the comment visible across rows: {wrapped:?}"
        );
        assert_eq!(layout.scroll, 0);
        assert!(
            layout.area.width <= MAX_WIDTH,
            "the popup must still cap its width"
        );
    }

    #[test]
    fn very_long_comment_caps_height_and_keeps_the_cursor_on_the_last_row() {
        let body = "word ".repeat(400);
        let layout = layout_comment_input(Rect::new(0, 0, 80, 24), body.trim_end());

        assert!(
            layout.area.height <= MAX_INNER_HEIGHT + 2,
            "an oversized comment must stop growing at the inner-height cap: {:?}",
            layout.area
        );
        assert!(
            layout.scroll > 0,
            "overflowing wrapped rows must scroll vertically to the end of the comment"
        );
        assert_eq!(
            layout.cursor.y,
            layout.area.y + layout.area.height - 2,
            "the cursor must stay on the last visible input row"
        );
    }

    #[test]
    fn render_puts_wrapped_words_on_separate_rows() {
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).expect("test terminal");
        let body = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
        terminal
            .draw(|frame| render(frame, body))
            .expect("comment input must render");

        let buffer = terminal.backend().buffer();
        let rows: Vec<String> = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect();
        let comment_rows: Vec<&str> = rows
            .iter()
            .map(String::as_str)
            .filter(|row| {
                row.contains("alpha")
                    || row.contains("kappa")
                    || row.contains("epsilon")
                    || row.contains("theta")
            })
            .collect();

        assert!(
            comment_rows.len() >= 2,
            "the rendered popup must wrap the long comment onto multiple rows, got {rows:?}"
        );
        assert!(
            rows.iter().any(|row| row.contains("Comment")),
            "the rendered popup must keep its title: {rows:?}"
        );
    }
}
