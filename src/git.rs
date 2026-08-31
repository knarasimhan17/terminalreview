use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

pub(crate) struct Repository {
    root: PathBuf,
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

        Ok(Self { root })
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

    fn diff_trees(&self, base_commit_sha: &str, tree_sha: &str) -> Result<String> {
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
