#!/usr/bin/env bash

set -euo pipefail

# Resolve paths and select rust-toolchain.toml from this repository, even when
# the script is invoked from another working directory.
repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
cd -- "$repo_root"

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
    printf 'Usage: %s [brinfo|focxt/call_chain|focxt|utgen ...]\n' "$0"
    printf 'Without arguments, install all four tools using the pinned toolchain.\n'
    exit 0
fi

if [[ $# -eq 0 ]]; then
    projects=("brinfo" "focxt/call_chain" "focxt" "utgen")
else
    projects=("$@")
fi

# Validate the full selection before starting any installation.
for project in "${projects[@]}"; do
    case "$project" in
        brinfo|focxt/call_chain|focxt|utgen) ;;
        *) printf 'Unknown tool: %s\nRun %s --help for usage.\n' "$project" "$0" >&2; exit 2 ;;
    esac
done

for project in "${projects[@]}"; do
    printf 'Installing %s...\n' "$project"
    if cargo install --path "$repo_root/$project" --locked; then
        printf '%s installed successfully.\n' "$project"
    else
        status=$?
        printf 'Failed to install %s (exit %s); see Cargo output above.\n' "$project" "$status" >&2
        exit "$status"
    fi
done
