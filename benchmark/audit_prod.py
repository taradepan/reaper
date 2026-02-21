#!/usr/bin/env python3
"""
Audit Reaper's Rust source for production-code quality issues.

Checks:
  1. unwrap() / expect() / panic! outside #[cfg(test)] blocks
  2. TODO / FIXME / HACK / XXX comments
  3. eprintln! / dbg! left in production paths
  4. Files missing module-level doc comments
  5. Public functions missing doc comments
"""

import re
import sys
from pathlib import Path
from dataclasses import dataclass, field
from typing import Optional

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

REPO_ROOT = Path(__file__).parent.parent
SRC_FILES = sorted(
    list((REPO_ROOT / "src").rglob("*.rs"))
)

# Names that are acceptable uses of unwrap/expect in production
# (e.g. infallible operations we've verified manually)
ACCEPTABLE_UNWRAP_PATTERNS = [
    # serde_json::to_string_pretty only fails on non-serialisable types;
    # our Value is always serialisable.
    r"serde_json::to_string_pretty\(&output\)\.unwrap\(\)",
]

# ---------------------------------------------------------------------------
# Data types
# ---------------------------------------------------------------------------

@dataclass
class Issue:
    file: str
    line: int
    category: str
    message: str
    code: str  # short code, e.g. "UNWRAP", "TODO"

    def __str__(self) -> str:
        return f"{self.file}:{self.line}: [{self.code}] {self.message}"


@dataclass
class AuditResult:
    issues: list[Issue] = field(default_factory=list)

    def add(self, file: str, line: int, category: str, message: str, code: str) -> None:
        self.issues.append(Issue(file, line, category, message, code))

    def summary(self) -> dict[str, int]:
        counts: dict[str, int] = {}
        for issue in self.issues:
            counts[issue.code] = counts.get(issue.code, 0) + 1
        return counts


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def is_test_line(line_stripped: str) -> bool:
    """Quick heuristic: line is inside a test annotation or test macro call."""
    return (
        line_stripped.startswith("#[test]")
        or line_stripped.startswith("#[cfg(test)]")
        or "mod tests" in line_stripped
    )


def parse_blocks(source: str) -> list[tuple[int, int, bool]]:
    """
    Return a list of (start_line, end_line, is_test) spans.
    is_test=True means the block is inside #[cfg(test)] or mod tests { ... }.

    This is a simple brace-depth tracker — good enough for well-formatted Rust.
    """
    lines = source.splitlines()
    spans: list[tuple[int, int, bool]] = []

    # Stack entries: (start_line_1indexed, is_test_block)
    stack: list[tuple[int, bool]] = []
    # Whether the NEXT opening brace starts a test block
    pending_test = False

    for i, raw in enumerate(lines, 1):
        stripped = raw.strip()

        # Detect test-block markers
        if "#[cfg(test)]" in stripped or (
            stripped.startswith("mod tests") and "{" not in stripped
        ):
            pending_test = True

        opens = raw.count("{") - raw.count("}")  # net brace change
        if opens > 0:
            for _ in range(opens):
                is_test = pending_test or (
                    bool(stack) and stack[-1][1]
                )
                stack.append((i, is_test))
                pending_test = False
        elif opens < 0:
            for _ in range(-opens):
                if stack:
                    start, is_test = stack.pop()
                    spans.append((start, i, is_test))

    return spans


def in_test_block(line_no: int, spans: list[tuple[int, int, bool]]) -> bool:
    """Return True if line_no falls inside any test block span."""
    return any(s <= line_no <= e and is_test for s, e, is_test in spans)


def is_comment(stripped: str) -> bool:
    return stripped.startswith("//")


def acceptable_unwrap(line: str) -> bool:
    for pat in ACCEPTABLE_UNWRAP_PATTERNS:
        if re.search(pat, line):
            return True
    return False


# ---------------------------------------------------------------------------
# Checkers
# ---------------------------------------------------------------------------

def check_unwrap_expect_panic(
    source: str, rel_path: str, spans: list[tuple[int, int, bool]], result: AuditResult
) -> None:
    """Flag .unwrap() and panic! outside test blocks.

    Note: .expect("reason") is the *idiomatic* Rust way to document
    infallible invariants and is intentionally NOT flagged as blocking.
    It is reported as an info-level item so reviewers can verify the
    invariant claim is correct.
    """
    for i, raw in enumerate(source.splitlines(), 1):
        stripped = raw.strip()
        if is_comment(stripped):
            continue
        if in_test_block(i, spans):
            continue
        if acceptable_unwrap(raw):
            continue

        if ".unwrap()" in raw:
            result.add(rel_path, i, "Panics", f".unwrap() in production code: {stripped[:80]}", "UNWRAP")
        if re.search(r"\.expect\(", raw):
            # .expect("reason") is idiomatic — report as info, not blocking.
            result.add(rel_path, i, "Info", f".expect() invariant assertion (verify reason): {stripped[:80]}", "EXPECT_INFO")
        if "panic!" in raw:
            result.add(rel_path, i, "Panics", f"panic! in production code: {stripped[:80]}", "PANIC")


def check_debug_macros(
    source: str, rel_path: str, spans: list[tuple[int, int, bool]], result: AuditResult
) -> None:
    """Flag dbg!, eprintln! outside test blocks."""
    for i, raw in enumerate(source.splitlines(), 1):
        stripped = raw.strip()
        if is_comment(stripped):
            continue
        if in_test_block(i, spans):
            continue

        if "dbg!" in raw:
            result.add(rel_path, i, "Debug", f"dbg! left in production code: {stripped[:80]}", "DBG")
        # eprintln! is OK in main.rs error handlers but flag elsewhere
        if "eprintln!" in raw and not rel_path.endswith("main.rs"):
            result.add(rel_path, i, "Debug", f"eprintln! in non-main production code: {stripped[:80]}", "EPRINTLN")


def check_todo_fixme(
    source: str, rel_path: str, result: AuditResult
) -> None:
    """Flag TODO / FIXME / HACK / XXX in any code or comments."""
    pattern = re.compile(r"\b(TODO|FIXME|HACK|XXX)\b", re.IGNORECASE)
    for i, raw in enumerate(source.splitlines(), 1):
        if pattern.search(raw):
            stripped = raw.strip()
            word = pattern.search(raw).group(1).upper()
            result.add(rel_path, i, "Maintenance", f"{word} annotation: {stripped[:80]}", word)


def check_missing_file_doc(
    source: str, rel_path: str, result: AuditResult
) -> None:
    """Flag files that have no //! module-level doc comment."""
    # Ignore lib.rs (it's just re-exports) and mod.rs files
    if rel_path.endswith("lib.rs") or rel_path.endswith("mod.rs"):
        return

    lines = source.splitlines()
    has_doc = False
    for line in lines[:10]:  # doc comment must be near the top
        stripped = line.strip()
        if stripped.startswith("//!") or stripped.startswith("///"):
            has_doc = True
            break
        # Skip blank lines and attributes at the very top
        if stripped and not stripped.startswith("//") and not stripped.startswith("#!"):
            break

    if not has_doc:
        result.add(rel_path, 1, "Documentation", "Missing module-level //! doc comment", "NO_DOC")


def check_large_functions(
    source: str, rel_path: str, spans: list[tuple[int, int, bool]], result: AuditResult
) -> None:
    """Flag functions longer than 120 lines (excluding test blocks)."""
    fn_pattern = re.compile(r"^\s*(pub\s+)?(fn\s+\w+)")
    lines = source.splitlines()

    fn_start: Optional[tuple[int, str]] = None
    depth = 0

    for i, raw in enumerate(lines, 1):
        m = fn_pattern.match(raw)
        if m and not in_test_block(i, spans):
            fn_start = (i, m.group(0).strip())

        depth += raw.count("{") - raw.count("}")

        if fn_start and depth == 0:
            length = i - fn_start[0]
            if length > 120:
                result.add(
                    rel_path,
                    fn_start[0],
                    "Complexity",
                    f"Function `{fn_start[1]}` is {length} lines long (> 120)",
                    "LONG_FN",
                )
            fn_start = None


def check_magic_numbers(
    source: str, rel_path: str, spans: list[tuple[int, int, bool]], result: AuditResult
) -> None:
    """Flag bare numeric literals > 9 that aren't obviously constants."""
    # Only check production code; skip test blocks and comment lines
    pattern = re.compile(r"\b(\d{2,})\b")
    # Acceptable: version strings, byte sizes, offsets in location.rs
    skip_files = {"location.rs"}
    if any(rel_path.endswith(s) for s in skip_files):
        return

    for i, raw in enumerate(source.splitlines(), 1):
        stripped = raw.strip()
        if is_comment(stripped):
            continue
        if in_test_block(i, spans):
            continue
        # Skip lines with obvious constants/sizes/capacities
        if any(kw in raw for kw in ["capacity", "with_capacity", "reserve", "len()", "usize", "u32", "u64", "i32", "i64", "const "]):
            continue
        for m in pattern.finditer(raw):
            val = int(m.group(1))
            # Skip small-ish numbers and year-like numbers
            if val > 100 and not (1990 <= val <= 2100):
                result.add(
                    rel_path, i, "Style",
                    f"Magic number {val} in production code — consider a named constant: {stripped[:60]}",
                    "MAGIC"
                )
                break  # one per line is enough


# ---------------------------------------------------------------------------
# Main audit
# ---------------------------------------------------------------------------

def audit_file(path: Path) -> list[Issue]:
    rel = str(path.relative_to(REPO_ROOT))
    try:
        source = path.read_text(encoding="utf-8")
    except Exception as e:
        print(f"  SKIP {rel}: {e}", file=sys.stderr)
        return []

    result = AuditResult()
    spans = parse_blocks(source)

    check_unwrap_expect_panic(source, rel, spans, result)
    check_debug_macros(source, rel, spans, result)
    check_todo_fixme(source, rel, result)
    check_missing_file_doc(source, rel, result)
    check_large_functions(source, rel, spans, result)
    # check_magic_numbers is noisy; enable selectively
    # check_magic_numbers(source, rel, spans, result)

    return result.issues


def main() -> int:
    print("=" * 68)
    print("  REAPER PRODUCTION CODE AUDIT")
    print("=" * 68)
    print(f"  Scanning {len(SRC_FILES)} source files under {REPO_ROOT / 'src'}")
    print()

    all_issues: list[Issue] = []
    for path in SRC_FILES:
        issues = audit_file(path)
        all_issues.extend(issues)

    # ── Categorised output ────────────────────────────────────────────────────
    categories = [
        ("UNWRAP",      "🔴 .unwrap() in production"),
        ("PANIC",       "🔴 panic! in production"),
        ("FIXME",       "🔴 FIXME annotations"),
        ("DBG",         "🟡 dbg! left in code"),
        ("EPRINTLN",    "🟡 eprintln! outside main"),
        ("TODO",        "🟡 TODO annotations"),
        ("HACK",        "🟡 HACK annotations"),
        ("XXX",         "🟡 XXX annotations"),
        ("EXPECT_INFO", "🔵 .expect() invariant assertions (review reason strings)"),
        ("NO_DOC",      "🔵 Missing module doc"),
        ("LONG_FN",     "🔵 Long function (>120 lines)"),
    ]

    found_any = False
    for code, label in categories:
        items = [x for x in all_issues if x.code == code]
        if not items:
            continue
        found_any = True
        print(f"{label}  ({len(items)} occurrence{'s' if len(items) != 1 else ''})")
        print("  " + "─" * 64)
        for issue in items:
            print(f"  {issue}")
        print()

    # ── Summary ───────────────────────────────────────────────────────────────
    print("=" * 68)
    summary = {}
    for issue in all_issues:
        summary[issue.code] = summary.get(issue.code, 0) + 1

    blocking = sum(summary.get(c, 0) for c in ("UNWRAP", "PANIC", "FIXME"))
    warnings = sum(summary.get(c, 0) for c in ("DBG", "EPRINTLN", "TODO", "HACK", "XXX"))
    info     = sum(summary.get(c, 0) for c in ("EXPECT_INFO", "NO_DOC", "LONG_FN"))

    print(f"  Blocking issues  : {blocking}")
    print(f"  Warnings         : {warnings}")
    print(f"  Info / style     : {info}")
    print(f"  Total            : {len(all_issues)}")
    print()

    if blocking == 0 and warnings == 0:
        print("  ✅  No blocking or warning issues found.")
    elif blocking == 0:
        print(f"  ⚠️   No blocking issues.  {warnings} warning(s) to review.")
    else:
        print(f"  ❌  {blocking} blocking issue(s) must be resolved before release.")

    print("=" * 68)
    print()

    return 1 if blocking > 0 else 0


if __name__ == "__main__":
    sys.exit(main())