#!/usr/bin/env bash
# PostToolUse (Edit|Write): remind that rules/src changes need a Reference page edit.
command -v jq >/dev/null 2>&1 || exit 0
f=$(jq -r '.tool_input.file_path // empty' 2>/dev/null) || exit 0
[ -n "$f" ] || exit 0
root=$(git rev-parse --show-toplevel 2>/dev/null) || root=""
rel="${f#"$root"/}"
case "$rel" in
  rules/*.toml|src/*.rs|src/*/*.rs|src/*/*/*.rs|src/*/*/*/*.rs)
    echo "Docs rule: this change to $rel needs the matching site/reference/*.json page edited in the same commit (then python3 tools/site_data.py)."
    # No cargo fmt here: the tree is not fmt-clean, so it would rewrite ~50 unrelated files.
    ;;
esac
exit 0
