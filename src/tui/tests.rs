use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::diff::ParsedDiff;
use crate::model::Side;

use super::{App, DiffLayout, DiffRow, Mode, footer_text};

const TWO_FILE_DIFF: &str = "\
diff --git first.rs first.rs
--- first.rs
+++ first.rs
@@ -1 +1 @@
-old
+new
diff --git second.rs second.rs
--- second.rs
+++ second.rs
@@ -1 +1 @@
-before
+after
";
const SIDE_COMMENT_DIFF: &str = "\
diff --git file.rs file.rs
--- file.rs
+++ file.rs
@@ -1,2 +1,2 @@
 shared
-old
+new
";

#[test]
fn inline_comment_toggle_preserves_the_selected_diff_line() {
    let mut app = App::new(ParsedDiff::parse(
        "\
diff --git file.rs file.rs
--- file.rs
+++ file.rs
@@ -1 +1 @@
-first
+second
",
    ));
    app.selected_diff = 1;
    let selected = app.selected_diff;

    assert!(app.inline_comments, "inline comments must start visible");
    assert!(
        footer_text(&app).contains("v inline comments: on"),
        "the footer must document the toggle and its state"
    );
    assert!(
        footer_text(&app).contains("? help"),
        "the review footer must make help discoverable"
    );

    app.handle_diff_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));

    assert!(!app.inline_comments, "v must hide inline comment bodies");
    assert_eq!(
        app.selected_diff, selected,
        "toggling inline comments must preserve the selected diff line"
    );
}

#[test]
fn help_closes_without_changing_review_state() {
    let mut app = App::new(ParsedDiff::parse(TWO_FILE_DIFF));
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    app.select_diff(2);
    let selected_diff = app.selected_diff;
    let collapsed_files = app.collapsed_files.clone();

    for close in [KeyCode::Char('?'), KeyCode::Esc, KeyCode::Char('q')] {
        app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert!(app.help, "? must open help in the review");
        app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        let outcome = app.handle_key(KeyEvent::new(close, KeyModifiers::NONE));

        assert!(outcome.is_none(), "closing help must not exit the review");
        assert!(!app.help, "a help close key must dismiss the overlay");
        assert_eq!(
            app.selected_diff, selected_diff,
            "help must preserve the selected diff row"
        );
        assert_eq!(
            app.collapsed_files, collapsed_files,
            "help must preserve file collapse state"
        );
        assert_eq!(
            app.diff_layout,
            DiffLayout::SideBySide,
            "help must preserve the active review layout"
        );
    }

    app.mode = Mode::Comments;
    app.handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
    let outcome = app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));

    assert!(
        outcome.is_none(),
        "q must close help instead of quitting from the comment list"
    );
    assert!(
        matches!(app.mode, Mode::Comments),
        "closing help must return to the comment list"
    );
}

#[test]
fn file_headers_toggle_bodies_and_remain_navigation_targets() {
    let mut app = App::new(ParsedDiff::parse(TWO_FILE_DIFF));
    let expanded_rows = app.diff_rows();

    app.handle_diff_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.diff_rows(),
        [
            DiffRow::File(0),
            DiffRow::File(1),
            DiffRow::Line { file: 1, line: 0 },
            DiffRow::Line { file: 1, line: 1 },
            DiffRow::Line { file: 1, line: 2 },
        ],
        "collapsing a header must hide only that file's body"
    );

    app.handle_diff_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    assert_eq!(
        app.selected_row(),
        Some(DiffRow::File(1)),
        "next-file navigation must still target collapsed section headers"
    );
    app.handle_diff_key(KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE));
    app.handle_diff_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    assert_eq!(
        app.diff_rows(),
        expanded_rows,
        "Tab on a header must expand the selected file without changing views"
    );

    app.select_diff(1);
    app.handle_diff_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(
        matches!(app.mode, Mode::Comments),
        "Tab on a code row must retain the existing comments-view shortcut"
    );
}

#[test]
fn side_by_side_comments_follow_the_active_column() {
    let mut app = App::new(ParsedDiff::parse(SIDE_COMMENT_DIFF));
    app.handle_diff_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    app.select_diff(2);

    app.handle_diff_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    app.start_comment();
    let Mode::CommentInput { body, .. } = &mut app.mode else {
        panic!("the old side of a context row must accept comments");
    };
    body.push_str("old side");
    app.handle_input_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    app.handle_diff_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    app.start_comment();
    let Mode::CommentInput { body, .. } = &mut app.mode else {
        panic!("the new side of a context row must accept comments");
    };
    body.push_str("new side");
    app.handle_input_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        app.comments
            .iter()
            .map(|comment| (comment.line, comment.side, comment.body.as_str()))
            .collect::<Vec<_>>(),
        [(1, Side::Old, "old side"), (1, Side::New, "new side")],
        "side-by-side comments must retain the active column in their anchors"
    );

    app.handle_diff_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(
        app.diff_layout,
        DiffLayout::Unified,
        "the s key must return to the session's unified view"
    );
    assert_eq!(
        app.comments_for_line(&app.diff.files[0].lines[1]).count(),
        2,
        "unified view must show comments anchored to either side of a context line"
    );

    app.selected_side = Side::Old;
    app.select_diff(4);
    app.handle_diff_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert!(
        matches!(app.selected_row(), Some(DiffRow::SideBySide { .. })),
        "switching layouts on a paired addition must retain its review row"
    );
    assert_eq!(
        app.selected_anchor().map(|anchor| anchor.side),
        Some(Side::New),
        "an addition must select the new column even after the old column was preferred"
    );
}
