#!/usr/bin/env bash
# Stop: warn if rules/ or src/ changed without any site/reference/ change.
root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
cd "$root" || exit 0
files=$( { git diff --name-only HEAD 2>/dev/null; git ls-files --others --exclude-standard 2>/dev/null; } )
echo "$files" | grep -Eq '^(rules|src)/' || exit 0
echo "$files" | grep -q '^site/reference/' && exit 0
echo "WARNING: rules/ or src/ changed but no site/reference/ page was edited; the docs rule requires it in the same commit."
exit 0
