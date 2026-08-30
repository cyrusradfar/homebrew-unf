#!/bin/bash
# Guards Rust file length and function length, with a baseline so existing
# code is never forced into a rewrite.
#
# Two measurements, one clippy run:
#
#   FILE LENGTH     production lines per file, limit 300. Rust has no native
#                   lint for this. Counts only what precedes the module-level
#                   `#[cfg(test)]` block -- counting inline unit tests would
#                   punish test coverage, which is backwards. The gap is not
#                   marginal: 31 files here are over 300 total lines, but only
#                   18 are over 300 production lines.
#
#   FUNCTION LENGTH clippy::too_many_lines, threshold from clippy.toml.
#                   Baselined as a COUNT PER FILE, not a line number, because
#                   line numbers move whenever anything above them changes.
#
# Fails on:
#   1. a file over a limit that is NOT in the baseline  (new debt)
#   2. a baseline entry that got worse                  (existing debt worsening)
#   3. a file you changed that is still in the baseline (touch it, fix it)
#
# THE EXTRACTION EXEMPTION, and why it exists:
#
# Extracting a helper out of a long function ADDS lines -- a signature, a doc
# comment, a call site -- while reducing complexity and function length. A naive
# "files may only shrink" rule fails that change, discouraging the exact
# refactoring this gate is meant to cause. It is not hypothetical: the first
# real use of this gate rejected a refactor that took filter.rs from 1 cognitive
# complexity violation to 0 and from 2 over-long functions to 1, purely because
# the moved doc comments added 41 lines.
#
# So a file may grow past its baseline when its over-long-function count fell.
# Trading lines for structure is the point.
#
# Rule 3 likewise asks a touched file to get BETTER, not to reach the limit in
# one commit. sentinel.rs is 904 production lines; demanding 300 in a single
# ticket is a rewrite, which is explicitly not what we are doing.
#
# Usage:
#   check-code-size.sh                     # rules 1 and 2
#   check-code-size.sh --changed <files>   # also rule 3
#   check-code-size.sh --write-baseline    # regenerate the baseline
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
BASELINE="$ROOT_DIR/.code-size-baseline"
LINE_LIMIT=300

# Production lines: everything before the module-level `#[cfg(test)]`. Uses the
# FIRST match, so a file with several test blocks is undercounted -- conservative
# in the only safe direction, since it can make a file look smaller but never
# flag one unfairly.
prod_lines() {
    local file="$1" marker
    marker=$(grep -n '^#\[cfg(test)\]' "$file" | head -1 | cut -d: -f1 || true)
    if [[ -n "$marker" ]]; then echo $((marker - 1)); else wc -l < "$file" | tr -d ' '; fi
}

# `--lib --bins` is deliberate and must match whatever --write-baseline used:
# `--lib` alone reports 27 over-long functions here and `--lib --bins` reports
# 28, so mixing the two invents a regression on an unchanged tree.
long_function_counts() {
    (cd "$ROOT_DIR" && cargo clippy -p unfudged --lib --bins -- -W clippy::too_many_lines 2>&1) \
        | grep -A1 'too many lines' | grep -- '-->' \
        | sed 's|.*--> ||; s|:[0-9]*:[0-9]*$||' | sort | uniq -c | awk '{print $2, $1}'
}

# One line per offending file: "<path> <prod_lines> <long_functions>"
snapshot() {
    local fns f lines n
    fns=$(long_function_counts)
    while IFS= read -r f; do
        local rel="${f#"$ROOT_DIR"/}"
        lines=$(prod_lines "$f")
        n=$(awk -v r="$rel" '$1 == r { print $2 }' <<< "$fns")
        n=${n:-0}
        if [[ "$lines" -gt "$LINE_LIMIT" || "$n" -gt 0 ]]; then
            echo "$rel $lines $n"
        fi
    done < <(find "$ROOT_DIR/src" -name '*.rs' | sort)
}

if [[ "${1:-}" == "--write-baseline" ]]; then
    snapshot > "$BASELINE"
    echo "Baseline written: $BASELINE"
    echo "  $(wc -l < "$BASELINE" | tr -d ' ') files"
    echo "  $(awk -v l="$LINE_LIMIT" '$2 > l' "$BASELINE" | wc -l | tr -d ' ') over ${LINE_LIMIT} production lines"
    echo "  $(awk '{s+=$3} END {print s+0}' "$BASELINE") over-long functions"
    exit 0
fi

[[ -f "$BASELINE" ]] || { echo "Error: no baseline at $BASELINE. Run: $0 --write-baseline"; exit 1; }

CURRENT=$(snapshot)
FAILED=0

while read -r file lines fns; do
    [[ -z "${file:-}" ]] && continue
    base_lines=$(awk -v f="$file" '$1 == f { print $2 }' "$BASELINE")
    base_fns=$(awk -v f="$file" '$1 == f { print $3 }' "$BASELINE")

    if [[ -z "$base_lines" ]]; then
        echo "FAIL  $file is new debt: ${lines} production lines, ${fns} over-long function(s)."
        echo "      New code must stay under ${LINE_LIMIT} lines per file and the function limit."
        FAILED=1
        continue
    fi

    if [[ "$fns" -gt "$base_fns" ]]; then
        echo "FAIL  $file went from ${base_fns} to ${fns} over-long function(s)."
        FAILED=1
    fi

    if [[ "$lines" -gt "$base_lines" ]]; then
        if [[ "$fns" -lt "$base_fns" ]]; then
            echo "note  $file grew ${base_lines} -> ${lines} lines, but over-long functions"
            echo "      fell ${base_fns} -> ${fns}. Allowed: that is extraction, not bloat."
        else
            echo "FAIL  $file grew from ${base_lines} to ${lines} production lines with no"
            echo "      structural improvement (over-long functions ${base_fns} -> ${fns})."
            FAILED=1
        fi
    fi
done <<< "$CURRENT"

# Rule 3: touch it, fix it. Asks for improvement, not instant compliance.
if [[ "${1:-}" == "--changed" ]]; then
    shift
    for file in "$@"; do
        [[ "$file" == src/*.rs ]] || continue
        [[ -f "$ROOT_DIR/$file" ]] || continue
        awk -v f="$file" '$1 == f { found = 1 } END { exit !found }' "$BASELINE" || continue

        base_lines=$(awk -v f="$file" '$1 == f { print $2 }' "$BASELINE")
        base_fns=$(awk -v f="$file" '$1 == f { print $3 }' "$BASELINE")
        now_lines=$(prod_lines "$ROOT_DIR/$file")
        now_fns=$(awk -v f="$file" '$1 == f { print $3 }' <<< "$CURRENT"); now_fns=${now_fns:-0}

        if [[ "$now_lines" -lt "$base_lines" || "$now_fns" -lt "$base_fns" ]]; then
            echo "ok    $file improved (${base_lines}->${now_lines} lines, ${base_fns}->${now_fns} long fns)."
            echo "      Update .code-size-baseline to lock the gain in."
        else
            echo "FAIL  $file is in the baseline and you changed it without improving it"
            echo "      (${base_lines}->${now_lines} lines, ${base_fns}->${now_fns} long fns)."
            echo "      Shorten it or extract a function. It need not reach ${LINE_LIMIT} today,"
            echo "      but it must move that way. Then update .code-size-baseline."
            FAILED=1
        fi
    done
fi

if [[ "$FAILED" -eq 0 ]]; then
    echo "Code size OK ($(wc -l < "$BASELINE" | tr -d ' ') files baselined, nothing new, nothing worse)"
fi
exit "$FAILED"
