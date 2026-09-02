use super::{DiffLineKind, FileChangeKind, ParsedDiff, SideBySideRow};

const DIFF: &str = "\
diff --git added.rs added.rs
new file mode 100644
index 0000000..1111111
--- /dev/null
+++ added.rs
@@ -0,0 +1 @@
+new
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
@@ -1,2 +1,2 @@ fn render()
-before
+after
--- code marker
+++ code marker
diff --git old.rs new.rs
similarity index 100%
rename from old.rs
rename to new.rs
";
const SIDE_BY_SIDE_DIFF: &str = "\
diff --git paired.rs paired.rs
--- paired.rs
+++ paired.rs
@@ -1,4 +1,4 @@
 shared
-old one
-old two
\\ No newline at end of file
+new one
 tail
+added
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
                vec![(DiffLineKind::Hunk, ""), (DiffLineKind::Addition, "new"),],
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
                2,
                2,
                vec![
                    (DiffLineKind::Hunk, "fn render()"),
                    (DiffLineKind::Deletion, "before"),
                    (DiffLineKind::Addition, "after"),
                    (DiffLineKind::Deletion, "-- code marker"),
                    (DiffLineKind::Addition, "++ code marker"),
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

#[test]
fn side_by_side_rows_pair_change_runs_within_each_hunk() {
    let parsed = ParsedDiff::parse(SIDE_BY_SIDE_DIFF);

    assert_eq!(
        parsed.files[0].side_by_side_rows(),
        [
            SideBySideRow::Hunk(0),
            SideBySideRow::Paired { old: 1, new: 1 },
            SideBySideRow::Paired { old: 2, new: 4 },
            SideBySideRow::Old(3),
            SideBySideRow::Paired { old: 5, new: 5 },
            SideBySideRow::New(6),
        ],
        "side-by-side rows must align each change run without crossing context boundaries"
    );
}
