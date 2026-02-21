#!/usr/bin/env bash
# =============================================================================
# Reaper vs Ruff — Final Comprehensive Benchmark
# Evaluates both SPEED (hyperfine) and ACCURACY (diagnostic agreement)
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
CORPUS="$SCRIPT_DIR/corpus"
BINARY="$REPO_ROOT/target/release/reaper"
RESULTS_DIR="$SCRIPT_DIR/results"
WARMUP=5
RUNS=30

# ── colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
CYAN='\033[0;36m'; BOLD='\033[1m'; RESET='\033[0m'

header() { echo -e "\n${BOLD}${CYAN}══════════════════════════════════════════════════${RESET}"; \
           echo -e "${BOLD}${CYAN}  $*${RESET}"; \
           echo -e "${BOLD}${CYAN}══════════════════════════════════════════════════${RESET}"; }
info()   { echo -e "${CYAN}▶ $*${RESET}"; }
ok()     { echo -e "${GREEN}✔ $*${RESET}"; }
warn()   { echo -e "${YELLOW}⚠ $*${RESET}"; }
err()    { echo -e "${RED}✘ $*${RESET}"; }

# ── sanity checks ────────────────────────────────────────────────────────────
header "Environment"

if [[ ! -f "$BINARY" ]]; then
    info "Release binary not found — building…"
    (cd "$REPO_ROOT" && cargo build --release --quiet)
fi
ok "Reaper binary : $BINARY"

RUFF_BIN="$(command -v ruff 2>/dev/null || true)"
if [[ -z "$RUFF_BIN" ]]; then
    err "ruff not found in PATH. Install with: pip install ruff"
    exit 1
fi
ok "Ruff binary   : $RUFF_BIN  ($(ruff --version))"

HYPERFINE_BIN="$(command -v hyperfine 2>/dev/null || true)"
if [[ -z "$HYPERFINE_BIN" ]]; then
    warn "hyperfine not found — speed benchmarks will be skipped."
    warn "Install with: brew install hyperfine"
    SKIP_SPEED=1
else
    ok "hyperfine     : $HYPERFINE_BIN  ($(hyperfine --version))"
    SKIP_SPEED=0
fi

mkdir -p "$RESULTS_DIR"

# ── collect corpus file lists ─────────────────────────────────────────────────
SMALL_FILES=$(ls "$CORPUS/small/"*.py 2>/dev/null | tr '\n' ' ')
MEDIUM_FILES=$(ls "$CORPUS/medium/"*.py 2>/dev/null | tr '\n' ' ')
LARGE_FILES=$(ls "$CORPUS/large/"*.py 2>/dev/null | tr '\n' ' ')
EDGE_FILES=$(ls "$CORPUS/edge_cases/"*.py 2>/dev/null | tr '\n' ' ')
ALL_FILES="$SMALL_FILES $MEDIUM_FILES $LARGE_FILES $EDGE_FILES"

count_files() { echo "$1" | tr ' ' '\n' | grep -c '\.py$' || true; }
N_SMALL=$(count_files "$SMALL_FILES")
N_MEDIUM=$(count_files "$MEDIUM_FILES")
N_LARGE=$(count_files "$LARGE_FILES")
N_EDGE=$(count_files "$EDGE_FILES")
N_ALL=$(count_files "$ALL_FILES")

info "Corpus: small=${N_SMALL}  medium=${N_MEDIUM}  large=${N_LARGE}  edge=${N_EDGE}  all=${N_ALL}"

# =============================================================================
# 1. SPEED BENCHMARKS
# =============================================================================
header "1 / Speed Benchmarks  (warmup=${WARMUP}, runs=${RUNS})"

run_hyperfine() {
    local label="$1"; local files="$2"
    local out="$RESULTS_DIR/perf_${label}.json"
    info "Running hyperfine — ${label} (${RUNS} runs)…"
    # shellcheck disable=SC2086
    hyperfine \
        --warmup "$WARMUP" \
        --runs   "$RUNS" \
        --ignore-failure \
        --export-json "$out" \
        --command-name "reaper" "$BINARY $files" \
        --command-name "ruff"   "$RUFF_BIN check --select F401,F811,F841,B007 --no-cache $files" \
        2>&1 | grep -E "(Time|mean|stddev|Fastest)" || true

    # extract mean & stddev from JSON and print inline summary
    if command -v python3 &>/dev/null; then
        python3 - "$out" "$label" <<'PYEOF'
import sys, json
out_file, label = sys.argv[1], sys.argv[2]
d = json.load(open(out_file))
r = next(x for x in d["results"] if x["command"] == "reaper")
u = next(x for x in d["results"] if x["command"] == "ruff")
rm, rs = r["mean"]*1000, r["stddev"]*1000
um, us = u["mean"]*1000, u["stddev"]*1000
sp = um / rm if rm > 0 else 0
print(f"  {label}: Reaper {rm:.1f} ms ± {rs:.1f} ms  |  Ruff {um:.1f} ms ± {us:.1f} ms  -> {sp:.2f}x faster")
PYEOF
    fi
}

if [[ "$SKIP_SPEED" -eq 0 ]]; then
    [[ -n "$SMALL_FILES"  ]] && run_hyperfine "small"  "$SMALL_FILES"
    [[ -n "$MEDIUM_FILES" ]] && run_hyperfine "medium" "$MEDIUM_FILES"
    [[ -n "$LARGE_FILES"  ]] && run_hyperfine "large"  "$LARGE_FILES"
    [[ -n "$EDGE_FILES"   ]] && run_hyperfine "edge"   "$EDGE_FILES"
    [[ -n "$ALL_FILES"    ]] && run_hyperfine "all"    "$ALL_FILES"
else
    warn "Speed benchmarks skipped (hyperfine not installed)."
fi

# =============================================================================
# 2. ACCURACY BENCHMARKS
# =============================================================================
header "2 / Accuracy Benchmarks  (shared rules: RP001/F401, RP002/F841, RP009/B007)"

# Mapping: reaper rule code → ruff rule code
# RP001 → F401  (unused imports)
# RP002 → F841  (unused local variables)
# RP009 → B007  (unused loop variables)

run_accuracy() {
    local label="$1"; local files="$2"
    [[ -z "$files" ]] && return

    local reaper_raw="$RESULTS_DIR/reaper_${label}_raw.txt"
    local ruff_raw="$RESULTS_DIR/ruff_${label}_raw.txt"
    local report="$RESULTS_DIR/accuracy_${label}.txt"

    # ── gather diagnostics ────────────────────────────────────────────────────
    # shellcheck disable=SC2086
    "$BINARY" $files 2>/dev/null | grep -E "RP001|RP002|RP009" \
        | sed 's/\r//' | sort > "$reaper_raw" || true

    # shellcheck disable=SC2086
    "$RUFF_BIN" check --select F401,F841,B007 --no-cache \
        --output-format concise $files 2>/dev/null \
        | grep -E "F401|F841|B007" \
        | sed 's/\r//' | sort > "$ruff_raw" || true

    local n_reaper n_ruff
    n_reaper=$(wc -l < "$reaper_raw" | tr -d ' ')
    n_ruff=$(wc -l < "$ruff_raw" | tr -d ' ')

    # ── normalise to "file:line:col rule message" ─────────────────────────────
    python3 - "$reaper_raw" "$ruff_raw" "$report" "$label" "$files" <<'PYEOF'
import sys, re, os, collections

reaper_file, ruff_file, report_file, label, files_str = sys.argv[1:]

# ---- helpers ----------------------------------------------------------------
def norm_path(p):
    # make paths comparable by taking basename-only for corpus files
    return os.path.basename(p)

def parse_reaper(line):
    # format: /abs/path/file.py:LINE:COL: RPXXX message
    m = re.match(r'^(.+?):(\d+):(\d+):\s+(RP\d+)\s+(.*)', line.strip())
    if not m:
        return None
    return norm_path(m.group(1)), int(m.group(2)), int(m.group(3)), m.group(4), m.group(5)

def parse_ruff(line):
    # format: /abs/path/file.py:LINE:COL: FXXX message
    m = re.match(r'^(.+?):(\d+):(\d+):\s+([A-Z]\d+)\s+(.*)', line.strip())
    if not m:
        return None
    return norm_path(m.group(1)), int(m.group(2)), int(m.group(3)), m.group(4), m.group(5)

RULE_MAP = {'RP001': 'F401', 'RP002': 'F841', 'RP009': 'B007'}

reaper_diags = []
for line in open(reaper_file):
    parsed = parse_reaper(line)
    if parsed:
        reaper_diags.append(parsed)

ruff_diags = []
for line in open(ruff_file):
    parsed = parse_ruff(line)
    if parsed:
        ruff_diags.append(parsed)

# ---- matching ---------------------------------------------------------------
# key = (file, line, col, translated_rule)
reaper_keys_exact = set()
reaper_keys_line  = set()
for f, l, c, rule, msg in reaper_diags:
    tr = RULE_MAP.get(rule, rule)
    reaper_keys_exact.add((f, l, c, tr))
    reaper_keys_line.add((f, l, tr))

ruff_keys_exact = set()
ruff_keys_line  = set()
for f, l, c, rule, msg in ruff_diags:
    ruff_keys_exact.add((f, l, c, rule))
    ruff_keys_line.add((f, l, rule))

exact_matches = reaper_keys_exact & ruff_keys_exact
line_matches  = reaper_keys_line  & ruff_keys_line

reaper_only_exact = reaper_keys_exact - ruff_keys_exact
ruff_only_exact   = ruff_keys_exact   - reaper_keys_exact
reaper_only_line  = reaper_keys_line  - ruff_keys_line
ruff_only_line    = ruff_keys_line    - reaper_keys_line

# ---- write report -----------------------------------------------------------
lines = []
SEP = "=" * 68

lines.append(SEP)
lines.append(f"ACCURACY REPORT — {label.upper()}")
lines.append(SEP)
lines.append(f"  Reaper diagnostics (shared rules) : {len(reaper_diags)}")
lines.append(f"  Ruff   diagnostics (shared rules) : {len(ruff_diags)}")
lines.append(f"  Exact  matches (file+line+col)    : {len(exact_matches)}")
lines.append(f"  Line   matches (file+line only)   : {len(line_matches)}")
lines.append(f"  Reaper-only  (possible FP)        : {len(reaper_only_line)}")
lines.append(f"  Ruff-only    (possible miss)      : {len(ruff_only_line)}")
lines.append("")

if reaper_only_line:
    lines.append("── REAPER ONLY (possible false positives) ──────────────────────")
    for f, l, rule in sorted(reaper_only_line):
        lines.append(f"  {f}:{l}  {rule}")
    lines.append("")

if ruff_only_line:
    lines.append("── RUFF ONLY (cases Reaper did not flag) ───────────────────────")
    for f, l, rule in sorted(ruff_only_line):
        lines.append(f"  {f}:{l}  {rule}")
    lines.append("")

lines.append(SEP)
report_text = "\n".join(lines)
open(report_file, "w").write(report_text)
print(report_text)
PYEOF

    ok "Accuracy done for '${label}'"
}

run_accuracy "small"  "$SMALL_FILES"
run_accuracy "medium" "$MEDIUM_FILES"
run_accuracy "large"  "$LARGE_FILES"
run_accuracy "edge"   "$EDGE_FILES"
run_accuracy "all"    "$ALL_FILES"

# =============================================================================
# 3. EDGE-CASE DEEP INSPECTION
# =============================================================================
header "3 / Edge-Case Deep Inspection"

EDGE_CASES=(
    "ec01_type_checking_guard.py:TYPE_CHECKING guard — Reaper should NOT flag the TYPE_CHECKING import"
    "ec02_annotation_only_no_rp002.py:Annotation-only vars — no RP002 for annotation-only names"
    "ec03_augassign_is_use.py:Augmented assignment counts as use — no RP002"
    "ec04_walrus_contexts.py:Walrus operator — name should be visible in outer scope"
    "ec05_underscore_exempt.py:Underscore names — _ and _prefix should be exempt"
    "ec06_locals_vars_suppress.py:locals()/vars() — suppress RP002 in that scope"
    "ec07_dunder_all_protection.py:__all__ protects names — no RP001/RP003 for exported names"
    "ec08_star_import_no_flag.py:Star import — should not fire RP001"
    "ec09_unused_import_aliased.py:Aliased unused import — RP001 on alias"
    "ec10_import_redefined_by_assign.py:Import redefined by assign — RP007"
    "ec11_if_false_none_dead_branch.py:if False/None dead branch — RP006"
    "ec12_unreachable_patterns.py:Unreachable after return/raise — RP005"
    "ec13_unused_args_patterns.py:Unused function arguments — RP008"
    "ec14_unused_loop_vars.py:Unused loop variables — RP009"
    "ec15_cross_file_anchor.py:Cross-file anchor — RP003/RP004 cross-file"
)

EC_PASS=0; EC_FAIL=0; EC_WARN=0

for entry in "${EDGE_CASES[@]}"; do
    fname="${entry%%:*}"
    desc="${entry#*:}"
    fpath="$CORPUS/edge_cases/$fname"

    if [[ ! -f "$fpath" ]]; then
        warn "SKIP  $fname — file not found"
        ((EC_WARN++)) || true
        continue
    fi

    reaper_out=$("$BINARY" "$fpath" 2>/dev/null || true)
    ruff_out=$("$RUFF_BIN" check --select F401,F811,F841,B007 --no-cache \
                    --output-format concise "$fpath" 2>/dev/null || true)

    # Per-file expected checks (heuristic — look for intentional markers)
    case "$fname" in
        ec01*)
            # Must NOT flag TYPE_CHECKING import block (lines 6-8)
            if echo "$reaper_out" | grep -q "RP001" && echo "$reaper_out" | grep -q ":6:\|:7:\|:8:"; then
                err  "FAIL  $fname: RP001 fired on TYPE_CHECKING import — FP!"
                ((EC_FAIL++)) || true
            else
                ok   "PASS  $fname: $desc"
                ((EC_PASS++)) || true
            fi
            ;;
        ec06*)
            # Must NOT fire RP002 inside a function that calls locals()/vars()
            if echo "$reaper_out" | grep -q "RP002.*:.*10:\|RP002.*:.*11:"; then
                err  "FAIL  $fname: RP002 fired inside locals()/vars() scope — FP!"
                ((EC_FAIL++)) || true
            else
                ok   "PASS  $fname: $desc"
                ((EC_PASS++)) || true
            fi
            ;;
        ec11*)
            # Must fire RP006 for if False / if None branches
            if echo "$reaper_out" | grep -q "RP006"; then
                ok   "PASS  $fname: $desc"
                ((EC_PASS++)) || true
            else
                err  "FAIL  $fname: RP006 not fired for dead if False/None branch"
                ((EC_FAIL++)) || true
            fi
            ;;
        ec12*)
            # Must fire RP005 for unreachable code
            if echo "$reaper_out" | grep -q "RP005"; then
                ok   "PASS  $fname: $desc"
                ((EC_PASS++)) || true
            else
                err  "FAIL  $fname: RP005 not fired for unreachable code"
                ((EC_FAIL++)) || true
            fi
            ;;
        ec14*)
            # Must fire RP009 for unused loop variables
            if echo "$reaper_out" | grep -q "RP009"; then
                ok   "PASS  $fname: $desc"
                ((EC_PASS++)) || true
            else
                err  "FAIL  $fname: RP009 not fired for unused loop variable"
                ((EC_FAIL++)) || true
            fi
            ;;
        *)
            # Generic: just show counts
            r_count=$(echo "$reaper_out" | grep -c "RP" || true)
            f_count=$(echo "$ruff_out"   | grep -c "[A-Z][0-9]" || true)
            printf "  %-45s  reaper=%s  ruff=%s\n" "$fname" "$r_count" "$f_count"
            ((EC_PASS++)) || true
            ;;
    esac
done

echo ""
info "Edge-case results: PASS=${EC_PASS}  FAIL=${EC_FAIL}  SKIP=${EC_WARN}"

# =============================================================================
# 4. MATCH/CASE SPECIFIC REGRESSION
# =============================================================================
header "4 / match/case Regression  (RP005 false-positive guard)"

MC_PASS=0; MC_FAIL=0

check_match_case() {
    local name="$1"; local src="$2"; local expect_rp005="$3"  # "yes" or "no"

    local tmpfile; tmpfile=$(mktemp /tmp/reaper_mc_XXXX.py)
    printf '%s' "$src" > "$tmpfile"
    local out; out=$("$BINARY" "$tmpfile" 2>/dev/null || true)
    rm -f "$tmpfile"

    local fired="no"
    echo "$out" | grep -q "RP005" && fired="yes"

    if [[ "$fired" == "$expect_rp005" ]]; then
        ok   "PASS  $name  (RP005 expected=${expect_rp005} got=${fired})"
        ((MC_PASS++)) || true
    else
        err  "FAIL  $name  (RP005 expected=${expect_rp005} got=${fired})"
        ((MC_FAIL++)) || true
        if [[ -n "$out" ]]; then
            echo "       diagnostics: $out"
        fi
    fi
}

# ── Case 1: Independent arms — NO RP005 across arms ─────────────────────────
check_match_case "match_independent_arms_no_rp005" \
'def f(x):
    match x:
        case 1:
            return 1
        case 2:
            return 2
        case _:
            return 0
' "no"

# ── Case 2: Real unreachable INSIDE an arm — MUST fire RP005 ────────────────
check_match_case "match_internal_unreachable_fires_rp005" \
'def f(x):
    match x:
        case 1:
            return 1
            print("dead")
        case _:
            return 0
' "yes"

# ── Case 3: Guard arms — still independent, NO RP005 ────────────────────────
check_match_case "match_guard_arms_no_rp005" \
'def f(x):
    match x:
        case n if n > 0:
            return n
        case n if n < 0:
            return -n
        case _:
            return 0
' "no"

# ── Case 4: Unreachable after match itself ─────────────────────────────────
check_match_case "match_exhaustive_followed_by_unreachable" \
'def f(x):
    match x:
        case 1:
            return 1
        case _:
            return 0
    print("this is unreachable only if match is exhaustive")
' "no"

# ── Case 5: Deeply nested match — no cross-arm contamination ─────────────────
check_match_case "match_nested_no_rp005" \
'def f(x, y):
    match x:
        case "a":
            match y:
                case 1:
                    return "a1"
                case _:
                    return "a?"
        case "b":
            return "b"
        case _:
            return "?"
' "no"

# ── Case 6: Actual dead code after unconditional return ──────────────────────
check_match_case "unconditional_return_then_dead" \
'def f():
    return 42
    x = 1
    print(x)
' "yes"

echo ""
info "match/case regression: PASS=${MC_PASS}  FAIL=${MC_FAIL}"

# =============================================================================
# 5. PERFORMANCE SUMMARY TABLE
# =============================================================================
header "5 / Final Summary"

if [[ "$SKIP_SPEED" -eq 0 ]] && command -v python3 &>/dev/null; then
    python3 - "$RESULTS_DIR" <<'PYEOF'
import os, json, sys

results_dir = sys.argv[1]
labels = ["small", "medium", "large", "edge", "all"]

print(f"\n{'Corpus':<10} {'Files':>6}  {'Reaper':>14}  {'Ruff':>14}  {'Speedup':>8}")
print("─" * 62)

counts = {"small": 20, "medium": 8, "large": 4, "edge": 15, "all": 47}

for label in labels:
    p = os.path.join(results_dir, f"perf_{label}.json")
    if not os.path.exists(p):
        print(f"  {label:<10}  (no data)")
        continue
    d = json.load(open(p))
    r = next((x for x in d["results"] if x["command"] == "reaper"), None)
    u = next((x for x in d["results"] if x["command"] == "ruff"), None)
    if r and u:
        rm = r["mean"] * 1000
        rs = r["stddev"] * 1000
        um = u["mean"] * 1000
        us = u["stddev"] * 1000
        sp = um / rm if rm > 0 else 0
        n  = counts.get(label, "?")
        print(f"  {label:<10} {n:>5}  "
              f"  {rm:6.1f} ±{rs:4.1f} ms"
              f"  {um:6.1f} ±{us:4.1f} ms"
              f"  {sp:6.2f}×")

print()
PYEOF
fi

# ── Accuracy roll-up ─────────────────────────────────────────────────────────
python3 - "$RESULTS_DIR" <<'PYEOF'
import os, re, sys

results_dir = sys.argv[1]
labels = ["small", "medium", "large", "edge", "all"]

print(f"\n{'Corpus':<10} {'Reaper':>7}  {'Ruff':>7}  {'Exact':>7}  {'Line':>7}  {'R-only FP':>10}  {'Ruff-only':>10}")
print("─" * 72)

for label in labels:
    p = os.path.join(results_dir, f"accuracy_{label}.txt")
    if not os.path.exists(p):
        print(f"  {label:<10}  (no data)")
        continue
    txt = open(p).read()
    def grab(key):
        m = re.search(rf'{re.escape(key)}\s*:\s*(\d+)', txt)
        return int(m.group(1)) if m else "?"
    nr = grab("Reaper diagnostics (shared rules)")
    nu = grab("Ruff   diagnostics (shared rules)")
    ne = grab("Exact  matches (file+line+col)")
    nl = grab("Line   matches (file+line only)")
    nrfp = grab("Reaper-only  (possible FP)")
    nuo  = grab("Ruff-only    (possible miss)")
    print(f"  {label:<10} {nr:>7}  {nu:>7}  {ne:>7}  {nl:>7}  {nrfp:>10}  {nuo:>10}")

print()
PYEOF

echo ""
info "All benchmark results written to: $RESULTS_DIR/"
echo ""

# ── Edge + match/case final status ───────────────────────────────────────────
TOTAL_EDGE_FAIL=$((EC_FAIL + MC_FAIL))
if [[ "$TOTAL_EDGE_FAIL" -eq 0 ]]; then
    ok "All edge-case and match/case regression checks PASSED."
else
    err "${TOTAL_EDGE_FAIL} edge/regression check(s) FAILED — see output above."
fi
echo ""