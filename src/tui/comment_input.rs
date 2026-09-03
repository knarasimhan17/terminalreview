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

pub(super) fn render(
    frame: &mut Frame<'_>,
    bounds: Rect,
    body: &str,
    selected_row: Option<Rect>,
    editing: bool,
) {
    let layout = layout_comment_input(bounds, body, selected_row);
    let title = if editing {
        " Edit comment "
    } else {
        " Comment "
    };
    frame.render_widget(Clear, layout.area);
    frame.render_widget(
        Paragraph::new(layout.text)
            .block(Block::bordered().title(title))
            .scroll((layout.scroll, 0)),
        layout.area,
    );
    if layout.area.width > 2 && layout.area.height > 2 {
        frame.set_cursor_position(layout.cursor);
    }
}

fn layout_comment_input(
    screen: Rect,
    body: &str,
    selected_row: Option<Rect>,
) -> CommentInputLayout {
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
    let area = place_comment_popup(screen, selected_row, width, height);
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

fn place_comment_popup(screen: Rect, selected_row: Option<Rect>, width: u16, height: u16) -> Rect {
    let width = width.min(screen.width);
    let height = height.min(screen.height);
    let Some(row) = selected_row.filter(|row| row.y < screen.bottom() && row.bottom() > screen.y)
    else {
        return centered(screen, width, height);
    };

    let max_x = screen.x.saturating_add(screen.width.saturating_sub(width));
    let x = row.x.saturating_add(2).clamp(screen.x, max_x);
    let below_y = row.y.saturating_add(1);
    let y = if below_y.saturating_add(height) <= screen.bottom() {
        below_y
    } else if row.y.saturating_sub(screen.y) >= height {
        row.y.saturating_sub(height)
    } else {
        screen.bottom().saturating_sub(height).max(screen.y)
    };

    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use unicode_width::UnicodeWidthStr;

    use super::{MAX_INNER_HEIGHT, MAX_WIDTH, layout_comment_input, place_comment_popup, render};

    fn inner_width(area: Rect) -> usize {
        usize::from(area.width.saturating_sub(2)).max(1)
    }

    #[test]
    fn short_comment_stays_one_line_tall() {
        let layout = layout_comment_input(Rect::new(0, 0, 80, 24), "looks good", None);
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
        let layout = layout_comment_input(Rect::new(0, 0, 40, 24), body, None);
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
        let layout = layout_comment_input(Rect::new(0, 0, 80, 24), body.trim_end(), None);

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
            .draw(|frame| render(frame, frame.area(), body, None, false))
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

    #[test]
    fn popup_sits_below_the_selected_line_when_there_is_room() {
        let screen = Rect::new(0, 0, 80, 24);
        let selected = Rect::new(1, 4, 78, 1);
        let area = place_comment_popup(screen, Some(selected), 36, 3);
        let centered = place_comment_popup(screen, None, 36, 3);

        assert_eq!(area.y, 5, "the popup must open on the row under the line");
        assert_eq!(area.x, 3, "the popup must indent from the selected line");
        assert_ne!(
            area.y, centered.y,
            "anchoring must not fall back to the screen center when the line has room below it"
        );
    }

    #[test]
    fn popup_sits_above_the_selected_line_near_the_bottom() {
        let screen = Rect::new(0, 0, 80, 24);
        let selected = Rect::new(1, 22, 78, 1);
        let area = place_comment_popup(screen, Some(selected), 36, 3);

        assert_eq!(
            area.y, 19,
            "a line near the bottom must open the popup above it: {area:?}"
        );
        assert_eq!(area.y + area.height, selected.y);
    }

    #[test]
    fn layout_follows_the_selected_line_instead_of_the_screen_center() {
        let screen = Rect::new(0, 0, 80, 24);
        let selected = Rect::new(1, 3, 78, 1);
        let anchored = layout_comment_input(screen, "note", Some(selected));
        let centered = layout_comment_input(screen, "note", None);

        assert_eq!(anchored.area.y, 4);
        assert_eq!(centered.area.y, 10);
        assert_ne!(anchored.area.y, centered.area.y);
    }
}
