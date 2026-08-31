#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
            sh -s -- -y --profile minimal
    elif command -v wget >/dev/null 2>&1; then
        wget -qO- https://sh.rustup.rs | sh -s -- -y --profile minimal
    else
        printf '%s\n' "cargo is missing and rustup requires curl or wget" >&2
        exit 1
    fi

    if [[ -f "$HOME/.cargo/env" ]]; then
        # rustup updates this file when the toolchain location changes.
        source "$HOME/.cargo/env"
    fi
fi

if ! command -v cargo >/dev/null 2>&1; then
    printf '%s\n' "cargo is unavailable after rustup installation" >&2
    exit 1
fi

cargo install --path "$script_dir"

if command -v tmux >/dev/null 2>&1; then
    exit 0
fi

if command -v brew >/dev/null 2>&1; then
    brew install tmux
    exit 0
fi

root_command=()
if [[ "$(id -u)" -ne 0 ]]; then
    if command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null; then
        root_command=(sudo -n)
    else
        printf '%s\n' "tmux is missing; skipping installation because sudo is unavailable" >&2
        exit 0
    fi
fi

if command -v apt-get >/dev/null 2>&1; then
    "${root_command[@]}" env DEBIAN_FRONTEND=noninteractive apt-get update
    "${root_command[@]}" env DEBIAN_FRONTEND=noninteractive apt-get install -y tmux
elif command -v dnf >/dev/null 2>&1; then
    "${root_command[@]}" dnf install -y tmux
elif command -v yum >/dev/null 2>&1; then
    "${root_command[@]}" yum install -y tmux
elif command -v pacman >/dev/null 2>&1; then
    "${root_command[@]}" pacman -Sy --noconfirm tmux
elif command -v apk >/dev/null 2>&1; then
    "${root_command[@]}" apk add tmux
else
    printf '%s\n' "tmux is missing; no supported package manager was found" >&2
fi
