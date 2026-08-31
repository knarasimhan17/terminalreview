# trv

`trv` is a terminal code-review tool for Git changes. It is designed around
vim-style navigation, inline comments, and immutable local review revisions. It
is built to close the loop with coding agents: an agent integration opens the
review when the agent finishes, and exported comments flow back automatically.

## Revisions

Each comment export creates a new immutable revision for the current repository
and branch. `trv` stores the revision metadata and an exact Git snapshot of the
reviewed code inside the repository's `.git` directory. The review data does not
modify tracked worktree files and is not included in ordinary branch pushes.

Revisions are numbered in export order. Creating rev-2 never changes rev-1, so
earlier reviews remain available for browsing with the code and comments that
belonged to them.

## Install

```sh
cargo install --path .
```

## Usage

```sh
trv
trv -r <revset>
trv revs
trv --stdout
```

`trv` reviews the current working tree against `HEAD`. `trv -r <revset>` reviews
a commit or revision range instead. Exporting comments creates the next
immutable revision for the current repository and branch.

`trv revs` lists the stored revisions. `trv --stdout` writes exported comments
to standard output instead of copying them to the clipboard, which lets an
agent workflow consume them directly.

Inside a review, use `j`/`k` to move between lines, `]`/`[` to move between
files, `g`/`G` to jump to the first or last line, `c` to add a comment, `l` to
view comments, `y` to export, and `q` to quit.
