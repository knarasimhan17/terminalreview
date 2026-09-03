use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};

// The picker targets recent work, and this caps both Git output and TUI memory.
const RECENT_COMMIT_LIMIT: usize = 200;

pub(crate) struct Repository {
    root: PathBuf,
    git_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CommitLogEntry {
    pub(crate) sha: String,
    pub(crate) short_sha: String,
    pub(crate) first_parent_sha: Option<String>,
    pub(crate) subject: String,
    pub(crate) committed_at: DateTime<Utc>,
    pub(crate) unpushed: bool,
}

pub(crate) struct PreparedReview {
    pub(crate) base_commit_sha: String,
    pub(crate) tree_sha: String,
    pub(crate) diff: String,
}

impl Repository {
    pub(crate) fn discover(start: &Path) -> Result<Self> {
        let root = PathBuf::from(
            git_text(start, &["rev-parse", "--show-toplevel"], None, None, false)
                .context("current directory is not inside a Git repository")?,
        );
        let common_dir = git_text(&root, &["rev-parse", "--git-common-dir"], None, None, false)?;
        let common_dir = PathBuf::from(common_dir);
        let git_dir = if common_dir.is_absolute() {
            common_dir
        } else {
            root.join(common_dir)
        };
        let git_dir = fs::canonicalize(&git_dir)
            .with_context(|| format!("failed to resolve {}", git_dir.display()))?;

        Ok(Self { root, git_dir })
    }

    pub(crate) fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub(crate) fn current_thread(&self) -> Result<String> {
        git_text(
            &self.root,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
            None,
            None,
            false,
        )
        .context("review revisions require a named branch")
    }

    pub(crate) fn recent_commits(&self) -> Result<Vec<CommitLogEntry>> {
        let limit = format!("--max-count={RECENT_COMMIT_LIMIT}");
        let output = git_bytes(
            &self.root,
            &[
                "log",
                "-z",
                &limit,
                "--format=%H%x00%h%x00%P%x00%ct%x00%s",
                "HEAD",
            ],
            None,
            None,
            false,
        )
        .context("failed to read recent commit log")?;
        let mut commits = parse_commit_log(&output)?;

        let output = git_text(
            &self.root,
            &["rev-list", &limit, "HEAD", "--not", "--remotes"],
            None,
            None,
            false,
        )
        .context("failed to detect commits absent from remote-tracking refs")?;
        let unpushed = output.lines().collect::<HashSet<_>>();
        for commit in &mut commits {
            commit.unpushed = unpushed.contains(commit.sha.as_str());
        }

        Ok(commits)
    }

    pub(crate) fn prepare_review(&self, revset: Option<&str>) -> Result<PreparedReview> {
        let (base_commit_sha, tree_sha) = match revset {
            Some(revset) => self.prepare_range(revset)?,
            None => self.capture_working_tree()?,
        };
        let diff = self.diff_trees(&base_commit_sha, &tree_sha)?;

        Ok(PreparedReview {
            base_commit_sha,
            tree_sha,
            diff,
        })
    }

    pub(crate) fn create_snapshot_commit(
        &self,
        tree_sha: &str,
        base_commit_sha: &str,
        revision: u64,
    ) -> Result<String> {
        let message = format!("trv rev-{revision}\n");
        git_text(
            &self.root,
            &["commit-tree", tree_sha, "-p", base_commit_sha],
            None,
            Some(message.as_bytes()),
            true,
        )
        .context("failed to create review snapshot commit")
    }

    pub(crate) fn create_revision_ref(
        &self,
        thread: &str,
        revision: u64,
        snapshot_commit_sha: &str,
    ) -> Result<()> {
        let reference = format!("refs/trv/{thread}/rev-{revision}");
        let missing_object = "0".repeat(snapshot_commit_sha.len());
        git_text(
            &self.root,
            &[
                "update-ref",
                &reference,
                snapshot_commit_sha,
                &missing_object,
            ],
            None,
            None,
            false,
        )
        .map(|_| ())
        .context("revision ref already exists or could not be created")
    }

    fn capture_working_tree(&self) -> Result<(String, String)> {
        let base_commit_sha = self.resolve_commit("HEAD").context(
            "working-tree review requires a HEAD commit; create the repository's first commit first",
        )?;
        let temporary =
            tempfile::tempdir().context("failed to create temporary Git index directory")?;
        let index = temporary.path().join("index");

        git_text(
            &self.root,
            &["read-tree", &base_commit_sha],
            Some(&index),
            None,
            false,
        )
        .context("failed to initialize temporary Git index")?;
        git_text(
            &self.root,
            &["add", "-A", "--", "."],
            Some(&index),
            None,
            false,
        )
        .context("failed to capture working tree in temporary Git index")?;
        let tree_sha = git_text(&self.root, &["write-tree"], Some(&index), None, false)
            .context("failed to write reviewed tree")?;

        Ok((base_commit_sha, tree_sha))
    }

    fn prepare_range(&self, revset: &str) -> Result<(String, String)> {
        if revset.is_empty() {
            bail!("revision range must not be empty");
        }

        let (base_commit_sha, snapshot_commit_sha) =
            if let Some((left, right)) = revset.split_once("...") {
                let left = default_head(left);
                let right = default_head(right);
                let left_sha = self.resolve_commit(left)?;
                let right_sha = self.resolve_commit(right)?;
                let base = git_text(
                    &self.root,
                    &["merge-base", &left_sha, &right_sha],
                    None,
                    None,
                    false,
                )
                .with_context(|| format!("failed to find merge base for {revset}"))?;
                (base, right_sha)
            } else if let Some((left, right)) = revset.split_once("..") {
                (
                    self.resolve_commit(default_head(left))?,
                    self.resolve_commit(default_head(right))?,
                )
            } else {
                let snapshot = self.resolve_commit(revset)?;
                let parent = format!("{snapshot}^");
                (self.resolve_commit(&parent)?, snapshot)
            };

        let tree_expression = format!("{snapshot_commit_sha}^{{tree}}");
        let tree_sha = git_text(
            &self.root,
            &["rev-parse", "--verify", &tree_expression],
            None,
            None,
            false,
        )
        .with_context(|| format!("failed to resolve reviewed tree for {revset}"))?;

        Ok((base_commit_sha, tree_sha))
    }

    pub(crate) fn commit_tree_sha(&self, commit: &str) -> Result<String> {
        let expression = format!("{commit}^{{tree}}");
        git_text(
            &self.root,
            &["rev-parse", "--verify", &expression],
            None,
            None,
            false,
        )
        .with_context(|| format!("failed to resolve tree for {commit}"))
    }

    fn resolve_commit(&self, revision: &str) -> Result<String> {
        let expression = format!("{revision}^{{commit}}");
        git_text(
            &self.root,
            &["rev-parse", "--verify", &expression],
            None,
            None,
            false,
        )
        .with_context(|| format!("failed to resolve commit {revision}"))
    }

    pub(crate) fn diff_trees(&self, base_commit_sha: &str, tree_sha: &str) -> Result<String> {
        let bytes = git_bytes(
            &self.root,
            &[
                "-c",
                "core.quotePath=false",
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--find-renames",
                "--unified=3",
                "--no-prefix",
                base_commit_sha,
                tree_sha,
                "--",
            ],
            None,
            None,
            false,
        )
        .context("failed to generate review diff")?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn parse_commit_log(output: &[u8]) -> Result<Vec<CommitLogEntry>> {
    let output = output.strip_suffix(b"\0").unwrap_or(output);
    if output.is_empty() {
        return Ok(Vec::new());
    }

    let fields = output.split(|byte| *byte == b'\0').collect::<Vec<_>>();
    let mut records = fields.chunks_exact(5);
    let mut commits = Vec::with_capacity(records.len());
    for record in &mut records {
        let parents = metadata_field(record[2], "commit parents")?;
        let timestamp = metadata_field(record[3], "commit timestamp")?
            .parse::<i64>()
            .context("Git returned an invalid commit timestamp")?;
        let committed_at = DateTime::<Utc>::from_timestamp(timestamp, 0).with_context(|| {
            format!("Git returned an out-of-range commit timestamp: {timestamp}")
        })?;

        commits.push(CommitLogEntry {
            sha: metadata_field(record[0], "commit SHA")?.to_owned(),
            short_sha: metadata_field(record[1], "short commit SHA")?.to_owned(),
            first_parent_sha: parents.split_ascii_whitespace().next().map(str::to_owned),
            subject: metadata_field(record[4], "commit subject")?.to_owned(),
            committed_at,
            unpushed: false,
        });
    }
    if !records.remainder().is_empty() {
        bail!(
            "Git commit log returned {} fields; expected groups of five",
            fields.len()
        );
    }

    Ok(commits)
}

fn metadata_field<'a>(field: &'a [u8], label: &str) -> Result<&'a str> {
    str::from_utf8(field).with_context(|| format!("Git returned non-UTF-8 {label}"))
}

fn default_head(revision: &str) -> &str {
    if revision.is_empty() {
        "HEAD"
    } else {
        revision
    }
}

fn git_text(
    directory: &Path,
    args: &[&str],
    index: Option<&Path>,
    input: Option<&[u8]>,
    fixed_identity: bool,
) -> Result<String> {
    let bytes = git_bytes(directory, args, index, input, fixed_identity)?;
    let text = String::from_utf8(bytes).context("Git returned non-UTF-8 metadata")?;
    Ok(text.trim_end_matches(['\r', '\n']).to_owned())
}

fn git_bytes(
    directory: &Path,
    args: &[&str],
    index: Option<&Path>,
    input: Option<&[u8]>,
    fixed_identity: bool,
) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    command.current_dir(directory).args(args);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    if fixed_identity {
        command
            .env("GIT_AUTHOR_NAME", "trv")
            .env("GIT_AUTHOR_EMAIL", "trv@localhost")
            .env("GIT_COMMITTER_NAME", "trv")
            .env("GIT_COMMITTER_EMAIL", "trv@localhost");
    }
    if input.is_some() {
        command.stdin(Stdio::piped());
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    let write_result = match input {
        Some(input) => match child.stdin.take() {
            Some(mut stdin) => stdin
                .write_all(input)
                .context("failed to write Git command input"),
            None => Err(anyhow::anyhow!(
                "Git stdin must be piped when command input is present"
            )),
        },
        None => Ok(()),
    };
    let output = child
        .wait_with_output()
        .context("failed to wait for Git command")?;
    write_result?;
    if output.status.success() {
        return Ok(output.stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "git {} exited with {}: {}",
        args.join(" "),
        output.status,
        stderr.trim()
    )
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};

    use super::{CommitLogEntry, Repository, git_text, parse_commit_log};

    #[test]
    fn commit_log_records_preserve_picker_metadata() {
        let output = b"1111111111111111111111111111111111111111\0\
1111111\0\
2222222222222222222222222222222222222222 3333333333333333333333333333333333333333\0\
1700000000\0\
feat: add picker\0\
2222222222222222222222222222222222222222\0\
2222222\0\
\0\
1690000000\0\
docs: initial commit\0";

        let commits =
            parse_commit_log(output).expect("well-formed Git log records must be parseable");

        assert_eq!(
            commits,
            vec![
                CommitLogEntry {
                    sha: "1111111111111111111111111111111111111111".to_owned(),
                    short_sha: "1111111".to_owned(),
                    first_parent_sha: Some("2222222222222222222222222222222222222222".to_owned()),
                    subject: "feat: add picker".to_owned(),
                    committed_at: DateTime::<Utc>::from_timestamp(1_700_000_000, 0)
                        .expect("the fixture timestamp must be representable"),
                    unpushed: false,
                },
                CommitLogEntry {
                    sha: "2222222222222222222222222222222222222222".to_owned(),
                    short_sha: "2222222".to_owned(),
                    first_parent_sha: None,
                    subject: "docs: initial commit".to_owned(),
                    committed_at: DateTime::<Utc>::from_timestamp(1_690_000_000, 0)
                        .expect("the fixture timestamp must be representable"),
                    unpushed: false,
                },
            ],
            "log parsing must preserve every field used by the picker"
        );
    }

    #[test]
    fn recent_commits_mark_only_history_absent_from_remote_tracking_refs() {
        let directory =
            tempfile::tempdir().expect("temporary repository creation must succeed for this test");
        run_git(directory.path(), &["init", "--quiet"]);
        run_git(
            directory.path(),
            &["commit", "--allow-empty", "--quiet", "-m", "pushed"],
        );
        let pushed = run_git(directory.path(), &["rev-parse", "HEAD"]);
        run_git(
            directory.path(),
            &["update-ref", "refs/remotes/origin/main", &pushed],
        );
        run_git(
            directory.path(),
            &["commit", "--allow-empty", "--quiet", "-m", "local"],
        );

        let repository = Repository::discover(directory.path())
            .expect("the synthetic repository must be discoverable");
        let commits = repository
            .recent_commits()
            .expect("recent commits must be readable from the synthetic repository");
        let status = commits
            .iter()
            .map(|commit| (commit.subject.as_str(), commit.unpushed))
            .collect::<Vec<_>>();

        assert_eq!(
            status,
            vec![("local", true), ("pushed", false)],
            "only commits unreachable from remote-tracking refs must be marked unpushed"
        );
    }

    fn run_git(directory: &std::path::Path, args: &[&str]) -> String {
        git_text(directory, args, None, None, true)
            .expect("synthetic repository Git commands must succeed")
    }
}
