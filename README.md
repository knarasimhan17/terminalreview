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

Run the setup script from the repository checkout:

```sh
./setup.sh
```

The script installs `trv`, bootstraps Rust when needed, and installs `tmux` when
a supported package manager is available. To install only the binary with an
existing Rust toolchain, run:

```sh
cargo install --path .
```

## Usage

```sh
trv
trv -w
trv -r <revset>
trv revs
trv --stdout
```

`trv` opens a picker with the working tree preselected, followed by the latest
200 commits on the current branch. Commits not found on any remote-tracking ref
are marked as unpushed. Selecting a commit opens a second picker for its base,
preselected to the commit's first parent.

`trv -w` (or `trv --working-tree`) reviews the current working tree against
`HEAD` directly. `trv -r <revset>` reviews a commit or revision range directly.
Both flags skip the picker. Exporting comments creates the next immutable
revision for the current repository and branch.

`trv revs` lists the stored revisions. `trv --stdout` writes exported comments
to standard output instead of copying them to the clipboard, which lets an
agent workflow consume them directly.

In the picker, use `j`/`k` to move, `Enter` to select, and `q` or `Esc` to go
back.

The review groups changes into file sections. Each file header shows its change
kind and added/deleted line counts; raw Git patch metadata is omitted. Files
start expanded. With a file header selected, use `Enter` or `Tab` to collapse or
expand it.

Inside a review, use `j`/`k` to move between lines, `]`/`[` to move between
files, `g`/`G` to jump to the first or last line, `c` to add a comment, `l` to
view comments, `s` to toggle unified or side-by-side layout, `v` to show or hide
inline comment rows, `y` to export, and `q` to quit. Unified layout is the
default. In side-by-side layout, use the left/right arrow keys to choose the old
or new side before adding a comment. Inline comments work in both layouts and
are shown by default; commented lines keep a `●` gutter marker when inline rows
are hidden.
