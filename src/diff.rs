use crate::model::Side;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffLineKind {
    Hunk,
    Addition,
    Deletion,
    Context,
    Meta,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileChangeKind {
    Modified,
    Added,
    Deleted,
    Renamed,
}

impl FileChangeKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Modified => "modified",
            Self::Added => "added",
            Self::Deleted => "deleted",
            Self::Renamed => "renamed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LineAnchor {
    pub(crate) path: String,
    pub(crate) line: u32,
    pub(crate) side: Side,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffLine {
    pub(crate) text: String,
    pub(crate) kind: DiffLineKind,
    pub(crate) old_line: Option<u32>,
    pub(crate) new_line: Option<u32>,
    old_anchor: Option<LineAnchor>,
    new_anchor: Option<LineAnchor>,
}

impl DiffLine {
    pub(crate) fn anchor(&self) -> Option<&LineAnchor> {
        self.new_anchor.as_ref().or(self.old_anchor.as_ref())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffFile {
    pub(crate) path: String,
    pub(crate) previous_path: Option<String>,
    pub(crate) change_kind: FileChangeKind,
    pub(crate) additions: usize,
    pub(crate) deletions: usize,
    pub(crate) lines: Vec<DiffLine>,
}

impl DiffFile {
    pub(crate) fn display_path(&self) -> String {
        match &self.previous_path {
            Some(previous_path) => format!("{previous_path} -> {}", self.path),
            None => self.path.clone(),
        }
    }
}

pub(crate) struct ParsedDiff {
    pub(crate) files: Vec<DiffFile>,
}

impl ParsedDiff {
    pub(crate) fn parse(diff: &str) -> Self {
        let mut files = Vec::new();
        let mut current = None;

        for text in diff.lines() {
            if let Some(header) = text.strip_prefix("diff --git ") {
                if let Some(file) = current.take() {
                    files.push(FileBuilder::finish(file));
                }
                let (old_path, new_path) = diff_header_paths(header);
                current = Some(FileBuilder::new(old_path, new_path));
                continue;
            }

            let Some(file) = &mut current else {
                continue;
            };
            if text.starts_with("new file mode ") {
                file.change_kind = FileChangeKind::Added;
            } else if text.starts_with("deleted file mode ") {
                file.change_kind = FileChangeKind::Deleted;
            } else if let Some(path) = text.strip_prefix("rename from ") {
                file.old_path = Some(clean_path(path));
                file.change_kind = FileChangeKind::Renamed;
            } else if let Some(path) = text.strip_prefix("rename to ") {
                file.new_path = Some(clean_path(path));
                file.change_kind = FileChangeKind::Renamed;
            } else if let Some(path) = text.strip_prefix("--- ") {
                file.old_path = diff_path(path);
                if file.old_path.is_none() {
                    file.change_kind = FileChangeKind::Added;
                }
            } else if let Some(path) = text.strip_prefix("+++ ") {
                file.new_path = diff_path(path);
                if file.new_path.is_none() {
                    file.change_kind = FileChangeKind::Deleted;
                }
            } else if text.starts_with("@@ ") {
                file.push_hunk(text);
            } else if let Some(content) = text.strip_prefix('+') {
                file.push_addition(content);
            } else if let Some(content) = text.strip_prefix('-') {
                file.push_deletion(content);
            } else if let Some(content) = text.strip_prefix(' ') {
                file.push_context(content);
            } else if text == r"\ No newline at end of file" {
                file.lines.push(diff_line(
                    "No newline at end of file",
                    DiffLineKind::Meta,
                    None,
                    None,
                    None,
                    None,
                ));
            } else if text.starts_with("Binary files ") {
                file.lines.push(diff_line(
                    "Binary content changed.",
                    DiffLineKind::Meta,
                    None,
                    None,
                    None,
                    None,
                ));
            }
        }

        if let Some(file) = current {
            files.push(FileBuilder::finish(file));
        }

        Self { files }
    }
}

struct FileBuilder {
    header_old_path: Option<String>,
    header_new_path: Option<String>,
    old_path: Option<String>,
    new_path: Option<String>,
    change_kind: FileChangeKind,
    additions: usize,
    deletions: usize,
    lines: Vec<DiffLine>,
    old_line: Option<u32>,
    new_line: Option<u32>,
}

impl FileBuilder {
    fn new(header_old_path: Option<String>, header_new_path: Option<String>) -> Self {
        Self {
            header_old_path,
            header_new_path,
            old_path: None,
            new_path: None,
            change_kind: FileChangeKind::Modified,
            additions: 0,
            deletions: 0,
            lines: Vec::new(),
            old_line: None,
            new_line: None,
        }
    }

    fn finish(self) -> DiffFile {
        let old_path = self.old_path.or(self.header_old_path);
        let new_path = self.new_path.or(self.header_new_path);
        let path = match self.change_kind {
            FileChangeKind::Deleted => old_path.clone(),
            FileChangeKind::Modified | FileChangeKind::Added | FileChangeKind::Renamed => {
                new_path.clone().or_else(|| old_path.clone())
            }
        }
        .unwrap_or_else(|| "(unknown file)".to_owned());
        let previous_path = if self.change_kind == FileChangeKind::Renamed
            && old_path.as_deref() != Some(path.as_str())
        {
            old_path
        } else {
            None
        };

        DiffFile {
            path,
            previous_path,
            change_kind: self.change_kind,
            additions: self.additions,
            deletions: self.deletions,
            lines: self.lines,
        }
    }

    fn push_hunk(&mut self, header: &str) {
        let (old_line, new_line) = hunk_starts(header);
        self.old_line = old_line;
        self.new_line = new_line;
        self.lines.push(diff_line(
            hunk_context(header),
            DiffLineKind::Hunk,
            None,
            None,
            None,
            None,
        ));
    }

    fn push_addition(&mut self, content: &str) {
        let displayed_new = self.new_line;
        let new_anchor = anchor(
            &self.new_path,
            &self.header_new_path,
            displayed_new,
            Side::New,
        );
        self.new_line = next_line(self.new_line);
        self.additions += 1;
        self.lines.push(diff_line(
            content,
            DiffLineKind::Addition,
            None,
            displayed_new,
            None,
            new_anchor,
        ));
    }

    fn push_deletion(&mut self, content: &str) {
        let displayed_old = self.old_line;
        let old_anchor = anchor(
            &self.old_path,
            &self.header_old_path,
            displayed_old,
            Side::Old,
        );
        self.old_line = next_line(self.old_line);
        self.deletions += 1;
        self.lines.push(diff_line(
            content,
            DiffLineKind::Deletion,
            displayed_old,
            None,
            old_anchor,
            None,
        ));
    }

    fn push_context(&mut self, content: &str) {
        let displayed_old = self.old_line;
        let displayed_new = self.new_line;
        let old_anchor = anchor(
            &self.old_path,
            &self.header_old_path,
            displayed_old,
            Side::Old,
        );
        let new_anchor = anchor(
            &self.new_path,
            &self.header_new_path,
            displayed_new,
            Side::New,
        );
        self.old_line = next_line(self.old_line);
        self.new_line = next_line(self.new_line);
        self.lines.push(diff_line(
            content,
            DiffLineKind::Context,
            displayed_old,
            displayed_new,
            old_anchor,
            new_anchor,
        ));
    }
}

fn diff_line(
    text: &str,
    kind: DiffLineKind,
    old_line: Option<u32>,
    new_line: Option<u32>,
    old_anchor: Option<LineAnchor>,
    new_anchor: Option<LineAnchor>,
) -> DiffLine {
    DiffLine {
        text: text.to_owned(),
        kind,
        old_line,
        new_line,
        old_anchor,
        new_anchor,
    }
}

fn diff_header_paths(header: &str) -> (Option<String>, Option<String>) {
    for (index, _) in header.match_indices(' ') {
        let old_path = &header[..index];
        let new_path = &header[index + 1..];
        if old_path == new_path {
            let path = clean_path(old_path);
            return (Some(path.clone()), Some(path));
        }
    }

    match header.split_once(' ') {
        Some((old_path, new_path)) => (Some(clean_path(old_path)), Some(clean_path(new_path))),
        None => (None, None),
    }
}

fn diff_path(path: &str) -> Option<String> {
    let path = clean_path(path);
    if path == "/dev/null" {
        None
    } else {
        Some(path)
    }
}

fn clean_path(path: &str) -> String {
    path.split('\t').next().unwrap_or(path).to_owned()
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

fn hunk_context(header: &str) -> &str {
    header
        .get(2..)
        .and_then(|rest| rest.find("@@").map(|end| &rest[end + 2..]))
        .map(str::trim)
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::{DiffLineKind, FileChangeKind, ParsedDiff};

    const DIFF: &str = "\
diff --git added.rs added.rs
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ added.rs
@@ -0,0 +1 @@
+new
\\ No newline at end of file
diff --git binary.bin binary.bin
index 5555555..6666666 100644
Binary files binary.bin and binary.bin differ
diff --git deleted.rs deleted.rs
deleted file mode 100644
index 2222222..0000000
--- deleted.rs
+++ /dev/null
@@ -1 +0,0 @@
-old
diff --git modified.rs modified.rs
index 3333333..4444444 100644
--- modified.rs
+++ modified.rs
@@ -1 +1 @@ fn render()
-before
+after
diff --git old.rs new.rs
similarity index 100%
rename from old.rs
rename to new.rs
";

    #[test]
    fn parsed_diff_groups_displayable_lines_and_file_summaries() {
        let parsed = ParsedDiff::parse(DIFF);
        let summaries = parsed
            .files
            .iter()
            .map(|file| {
                (
                    file.display_path(),
                    file.change_kind,
                    file.additions,
                    file.deletions,
                    file.lines
                        .iter()
                        .map(|line| (line.kind, line.text.as_str()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            summaries,
            vec![
                (
                    "added.rs".to_owned(),
                    FileChangeKind::Added,
                    1,
                    0,
                    vec![
                        (DiffLineKind::Hunk, ""),
                        (DiffLineKind::Addition, "new"),
                        (DiffLineKind::Meta, "No newline at end of file"),
                    ],
                ),
                (
                    "binary.bin".to_owned(),
                    FileChangeKind::Modified,
                    0,
                    0,
                    vec![(DiffLineKind::Meta, "Binary content changed.")],
                ),
                (
                    "deleted.rs".to_owned(),
                    FileChangeKind::Deleted,
                    0,
                    1,
                    vec![(DiffLineKind::Hunk, ""), (DiffLineKind::Deletion, "old"),],
                ),
                (
                    "modified.rs".to_owned(),
                    FileChangeKind::Modified,
                    1,
                    1,
                    vec![
                        (DiffLineKind::Hunk, "fn render()"),
                        (DiffLineKind::Deletion, "before"),
                        (DiffLineKind::Addition, "after"),
                    ],
                ),
                (
                    "old.rs -> new.rs".to_owned(),
                    FileChangeKind::Renamed,
                    0,
                    0,
                    vec![],
                ),
            ],
            "parsed files must contain review rows and summaries without raw patch metadata"
        );
    }
}
