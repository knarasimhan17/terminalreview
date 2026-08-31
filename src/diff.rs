use crate::model::Side;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffLineKind {
    File,
    Hunk,
    Addition,
    Deletion,
    Context,
    Meta,
}

#[derive(Clone, Debug)]
pub(crate) struct LineAnchor {
    pub(crate) path: String,
    pub(crate) line: u32,
    pub(crate) side: Side,
}

#[derive(Clone, Debug)]
pub(crate) struct DiffLine {
    pub(crate) text: String,
    pub(crate) kind: DiffLineKind,
    pub(crate) old_line: Option<u32>,
    pub(crate) new_line: Option<u32>,
    anchor: Option<LineAnchor>,
}

impl DiffLine {
    pub(crate) fn anchor(&self) -> Option<&LineAnchor> {
        self.anchor.as_ref()
    }
}

pub(crate) struct ParsedDiff {
    pub(crate) lines: Vec<DiffLine>,
    pub(crate) file_starts: Vec<usize>,
}

impl ParsedDiff {
    pub(crate) fn parse(diff: &str) -> Self {
        let mut lines = Vec::new();
        let mut file_starts = Vec::new();
        let mut old_path = None;
        let mut new_path = None;
        let mut old_line = None;
        let mut new_line = None;

        for text in diff.lines() {
            if text.starts_with("diff --git ") {
                file_starts.push(lines.len());
                old_path = None;
                new_path = None;
                old_line = None;
                new_line = None;
                lines.push(diff_line(text, DiffLineKind::File, None, None, None));
                continue;
            }
            if let Some(path) = text.strip_prefix("--- ") {
                old_path = diff_path(path);
                lines.push(diff_line(text, DiffLineKind::Meta, None, None, None));
                continue;
            }
            if let Some(path) = text.strip_prefix("+++ ") {
                new_path = diff_path(path);
                lines.push(diff_line(text, DiffLineKind::Meta, None, None, None));
                continue;
            }
            if text.starts_with("@@ ") {
                let (parsed_old, parsed_new) = hunk_starts(text);
                old_line = parsed_old;
                new_line = parsed_new;
                lines.push(diff_line(text, DiffLineKind::Hunk, None, None, None));
                continue;
            }

            if text.starts_with('+') {
                let displayed_new = new_line;
                let anchor = anchor(&new_path, &old_path, displayed_new, Side::New);
                new_line = next_line(new_line);
                lines.push(diff_line(
                    text,
                    DiffLineKind::Addition,
                    None,
                    displayed_new,
                    anchor,
                ));
            } else if text.starts_with('-') {
                let displayed_old = old_line;
                let anchor = anchor(&old_path, &new_path, displayed_old, Side::Old);
                old_line = next_line(old_line);
                lines.push(diff_line(
                    text,
                    DiffLineKind::Deletion,
                    displayed_old,
                    None,
                    anchor,
                ));
            } else if text.starts_with(' ') {
                let displayed_old = old_line;
                let displayed_new = new_line;
                let anchor = anchor(&new_path, &old_path, displayed_new, Side::New);
                old_line = next_line(old_line);
                new_line = next_line(new_line);
                lines.push(diff_line(
                    text,
                    DiffLineKind::Context,
                    displayed_old,
                    displayed_new,
                    anchor,
                ));
            } else {
                lines.push(diff_line(text, DiffLineKind::Meta, None, None, None));
            }
        }

        if lines.is_empty() {
            lines.push(diff_line(
                "No changes to review.",
                DiffLineKind::Meta,
                None,
                None,
                None,
            ));
        }

        Self { lines, file_starts }
    }
}

fn diff_line(
    text: &str,
    kind: DiffLineKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    anchor: Option<LineAnchor>,
) -> DiffLine {
    DiffLine {
        text: text.to_owned(),
        kind,
        old_line,
        new_line,
        anchor,
    }
}

fn diff_path(path: &str) -> Option<String> {
    if path == "/dev/null" {
        None
    } else {
        Some(path.to_owned())
    }
}

fn hunk_starts(header: &str) -> (Option<u32>, Option<u32>) {
    let mut old = None;
    let mut new = None;
    for field in header.split_ascii_whitespace() {
        if let Some(range) = field.strip_prefix('-') {
            old = range_start(range);
        } else if let Some(range) = field.strip_prefix('+') {
            new = range_start(range);
        }
    }
    (old, new)
}

fn range_start(range: &str) -> Option<u32> {
    range.split(',').next()?.parse().ok()
}

fn anchor(
    preferred_path: &Option<String>,
    fallback_path: &Option<String>,
    line: Option<u32>,
    side: Side,
) -> Option<LineAnchor> {
    Some(LineAnchor {
        path: preferred_path.as_ref().or(fallback_path.as_ref())?.clone(),
        line: line?,
        side,
    })
}

fn next_line(line: Option<u32>) -> Option<u32> {
    line.and_then(|line| line.checked_add(1))
}
