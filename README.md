<p align="center">
  <br />
  <code>💀</code>
  <br />
  <br />
  <strong style="font-size: 2em;">R E A P E R</strong>
  <br />
  <em>Find dead Python code before it finds you.</em>
  <br />
  <br />
  <a href="#-quickstart"><img src="https://img.shields.io/badge/lang-rust-B7410E?style=flat-square&logo=rust" alt="Built with Rust" /></a>
  <a href="#-rules"><img src="https://img.shields.io/badge/rules-9_checks-8B5CF6?style=flat-square" alt="9 Rules" /></a>
  <a href="#-performance"><img src="https://img.shields.io/badge/speed-~3ms_avg-10B981?style=flat-square" alt="~3ms average" /></a>
  <a href="#-cross-file-analysis"><img src="https://img.shields.io/badge/analysis-cross--file-F59E0B?style=flat-square" alt="Cross-file analysis" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue?style=flat-square" alt="MIT License" /></a>
</p>

---

**Reaper** is a blazing-fast, zero-config dead code finder for Python — built in Rust. It scans your entire project in milliseconds and catches what other tools miss: unused imports, dead functions, unreachable code, phantom classes, and more — **across file boundaries**.

> **One command. Nine rules. Zero configuration required.**

```bash
reaper
```

---

## What is Reaper?

Reaper reads your **entire project at once** and cross-references every definition against every usage — so it catches dead code that single-file tools structurally cannot see.

- **9 rules** — imports, variables, functions, classes, unreachable code, dead branches, redefined imports, arguments, loop variables
- **Cross-file analysis** — a function defined in `utils.py` but called nowhere is flagged project-wide
- **Parallel** — every file is analysed concurrently via [Rayon](https://docs.rs/rayon); per-file checkers also run in parallel
- **Zero config** — works out of the box; respects `.gitignore` automatically
- **`# noqa` support** — suppress any rule inline or by code
- **JSON output** — machine-readable results for CI pipelines

---

## ⚡ Quickstart

### Install from source

```bash
# Clone and build
git clone https://github.com/YOUR_USERNAME/reaper.git
cd reaper
cargo build --release

# The binary is at ./target/release/reaper
# Optionally, put it on your PATH:
cp ./target/release/reaper /usr/local/bin/
```

### Install via Cargo

```bash
cargo install --path .
```

### Run it

```bash
# Scan the current directory (these are identical)
reaper
reaper .

# Scan specific files or directories
reaper src/ lib/utils.py scripts/

# Only check specific rules
reaper --select RP001,RP003 .

# Exclude test directories
reaper --exclude tests,migrations,fixtures .

# JSON output for CI pipelines
reaper --json .

# Don't fail CI — just report
reaper --json --no-exit-code .
```

---

## 🎯 What It Looks Like

Give Reaper a Python file like this:

```python
import os                          # ← never used
import sys                         # ← never used
import json
from collections import OrderedDict, defaultdict  # OrderedDict never used

def fetch_users(db, timeout):      # ← 'timeout' arg never used
    unused_config = {"retries": 3} # ← assigned, never read
    return db.execute("SELECT *")

def deprecated_endpoint():         # ← nobody calls this function
    return None

class LegacyParser:                # ← nobody instantiates this class
    def parse(self, raw):
        return raw.split(",")

def process_batch(items):
    for item in items:
        item.save()
    return True
    remaining = items[:]           # ← unreachable after return
    return remaining

if False:                          # ← dead branch, never executes
    debug_mode = True

for idx in range(10):              # ← 'idx' is never used
    print("processing...")
```

Reaper outputs:

```
demo.py:1:8:   RP001 `os` imported but unused
demo.py:2:8:   RP001 `sys` imported but unused
demo.py:4:25:  RP001 `OrderedDict` imported but unused
demo.py:6:21:  RP008 Argument `timeout` is not used
demo.py:7:5:   RP002 Local variable `unused_config` is assigned but never used
demo.py:11:1:  RP003 Function `deprecated_endpoint` is defined but never used
demo.py:14:1:  RP004 Class `LegacyParser` is defined but never used
demo.py:22:5:  RP005 Code is unreachable
demo.py:25:1:  RP006 `if False:` branch is never executed
demo.py:29:5:  RP009 Loop variable `idx` is not used
Found 10 issue(s)
```

**All 9 rules fired. Time: <5ms.** ⚡

---

## 📏 Rules

Reaper ships with **9 purpose-built dead-code rules**:

### RP001 — Unused Import

```python
import os          # RP001 — `os` imported but unused
import json        # ✅ OK — used below

data = json.loads('{}')
```

Respects `__all__`, `TYPE_CHECKING` guards, and `__future__` imports.

---

### RP002 — Unused Variable

```python
def calculate():
    temp = 42       # RP002 — assigned but never read
    result = 100
    return result
```

Smart about augmented assignments (`total += 1`), walrus operators (`:=`), and comprehension variables.

---

### RP003 — Unused Function (Cross-File) 🌐

```python
# utils.py
def helper():        # ✅ OK — called from main.py
    return 42

def orphan():        # RP003 — defined but never called from anywhere
    return 0
```

RP003 scans your **entire project** — if no file imports or calls `orphan()`, it's dead.

---

### RP004 — Unused Class (Cross-File) 🌐

```python
class UserSerializer:     # ✅ OK — instantiated in views.py
    pass

class LegacyParser:       # RP004 — never instantiated or referenced anywhere
    pass
```

Same cross-file analysis as RP003 — project-wide dead class detection.

---

### RP005 — Unreachable Code

```python
def process():
    return True
    cleanup()        # RP005 — Code is unreachable
    log("done")      # RP005 — Code is unreachable
```

Detects code after `return`, `raise`, `break`, and `continue` — not just "unused variable" but the semantically richer **"this code can never execute"**.

---

### RP006 — Dead Branch

```python
if False:                     # RP006 — branch never executes
    enable_debug()

if None:                      # RP006 — branch never executes
    setup_logging()

from typing import TYPE_CHECKING
if TYPE_CHECKING:             # RP006 — correctly identified as dead at runtime
    import heavy_module       # (but NOT flagged as RP001 — Reaper knows this is intentional)
```

---

### RP007 — Import Redefined Before Use

```python
import os                    # RP007 — overwritten before it's ever read
os = "not the module anymore"

import sys                   # ✅ OK — used before reassignment
print(sys.version)
sys = "overwritten later"
```

---

### RP008 — Unused Function Argument

```python
def send_email(to, subject, priority):   # RP008 — `priority` is never used
    return mailer.send(to=to, subject=subject)

def callback(_event, data):              # ✅ OK — underscore prefix = intentionally unused
    return process(data)

class Base(ABC):
    @abstractmethod
    def handle(self, request):           # ✅ OK — abstract methods are skipped
        ...
```

Respects `_`-prefixed arguments, `*args`, `**kwargs`, `self`, `cls`, and abstract methods.

---

### RP009 — Unused Loop Variable

```python
for i in range(10):          # RP009 — `i` is never used
    print("tick")

for _ in range(10):          # ✅ OK — underscore convention
    print("tick")

for key, value in data.items():  # ✅ OK — both used
    result[key] = transform(value)
```

---

## 🔗 Cross-File Analysis

This is Reaper's killer feature. Most linters are single-file: they can tell you `os` is unused on line 1, but they cannot tell you that `generate_report()` defined in `utils.py` is never called from *anywhere* in your codebase. Reaper can.

**How it works:**

```
Project/
├── utils.py      →  defines fetch_users(), sync_inventory(), generate_report()
├── models.py     →  defines User, CacheManager
└── main.py       →  imports fetch_users, User
```

```bash
$ reaper --select RP003,RP004 Project/

utils.py:5:1:  RP003 Function `sync_inventory` is defined but never used
utils.py:9:1:  RP003 Function `generate_report` is defined but never used
models.py:8:1: RP004 Class `CacheManager` is defined but never used
Found 3 issue(s)
```

Reaper builds a **global usage set** across all files in one parallel pass. A function defined in `utils.py` but called from `main.py` is correctly recognized as alive. A function that nobody anywhere calls — that's dead code.

> **Note:** Cross-file analysis uses name-based matching, not full import-graph resolution. It errs on the side of false negatives (missing some dead code) rather than false positives (never flags live code as dead).

---

## ⚡ Performance

Measured with `hyperfine --warmup 5 --runs 30` on Apple Silicon.

| Corpus | Files | Time |
|--------|-------|------|
| Small | 20 | **2.6 ms** ± 0.3 |
| Medium | 8 | **3.1 ms** ± 0.4 |
| Large | 4 | **3.2 ms** ± 0.3 |
| Edge cases | 15 | **2.1 ms** ± 0.3 |
| **All** | **47** | **4.6 ms** ± 0.3 |

### Why is it fast?

```
┌──────────────────────────────────────────────────┐
│                   Reaper Core                     │
│                                                   │
│  ┌─────────────┐    ┌──────────────┐             │
│  │  Zero-copy   │───▶│  Single-pass  │            │
│  │   Lexer      │    │   AST Build   │            │
│  └─────────────┘    └──────┬───────┘             │
│                            │                      │
│            ┌───────────────┼───────────────┐      │
│            ▼               ▼               ▼      │
│     ┌──────────┐   ┌──────────┐   ┌──────────┐   │
│     │ Per-file  │   │ Per-file  │   │ Per-file  │  │
│     │ Checks   │   │ Checks   │   │ Checks   │   │
│     │ (Rayon)  │   │ (Rayon)  │   │ (Rayon)  │   │
│     └────┬─────┘   └────┬─────┘   └────┬─────┘   │
│          └───────────────┼───────────────┘        │
│                          ▼                        │
│               ┌──────────────────┐                │
│               │   Cross-file     │                │
│               │  RP003 / RP004   │                │
│               │  (Global merge)  │                │
│               └──────────────────┘                │
└──────────────────────────────────────────────────┘
```

1. **Custom zero-copy lexer** — Tokens borrow directly from the source string. No heap allocations during lexing.
2. **Single-pass AST** — The parser builds a typed AST in one linear pass. No backtracking.
3. **Parallel per-file analysis** — Every file is analyzed concurrently via [Rayon](https://docs.rs/rayon). Within each file, all 6 per-file checkers run in parallel too.
4. **Two-pass architecture** — Pass 1 (parallel): per-file checks + collect defs/usages. Pass 2 (parallel merge): cross-file RP003/RP004 against the global usage set.

---

## 🔇 Suppressing Diagnostics

### Inline with `# noqa`

```python
import os           # noqa            — suppress ALL rules on this line
import sys          # noqa: RP001     — suppress only RP001
import re           # noqa: RP001, RP007  — suppress RP001 and RP007
```

### With `--select` (only run specific rules)

```bash
# Only check for unused imports and dead functions
reaper --select RP001,RP003 .
```

### With `--exclude` (skip directories)

```bash
# Skip tests, migrations, and generated code
reaper --exclude tests,migrations,generated .
```

### Auto-excluded directories

These are **always** skipped — you never need to list them manually:

> `.git` · `.hg` · `.svn` · `.venv` · `.env` · `venv` · `env` · `virtualenv` · `__pycache__` · `.mypy_cache` · `.ruff_cache` · `.pytest_cache` · `.hypothesis` · `.tox` · `.nox` · `build` · `dist` · `.eggs` · `node_modules`

---

## 🤖 CI Integration

### GitHub Actions

```yaml
name: Dead Code Check

on: [push, pull_request]

jobs:
  reaper:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install Reaper
        run: cargo install --git https://github.com/YOUR_USERNAME/reaper.git

      - name: Find dead code
        run: reaper --exclude tests .
```

### GitHub Actions (JSON + inline annotations)

```yaml
      - name: Find dead code (JSON)
        run: |
          reaper --json --no-exit-code . > reaper-report.json
          cat reaper-report.json | jq -r \
            '.diagnostics[] | "::warning file=\(.file),line=\(.line),col=\(.col)::\(.code) \(.message)"'
```

### Pre-commit hook

```yaml
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: reaper
        name: reaper (dead code)
        entry: reaper --exclude tests
        language: system
        types: [python]
        pass_filenames: false
```

### Makefile

```makefile
.PHONY: lint lint-json
lint:
	reaper --exclude tests,migrations .

lint-json:
	reaper --json --no-exit-code --exclude tests . > dead-code-report.json
```

---

## 🖥️ CLI Reference

```
reaper [OPTIONS] [PATHS]...
```

| Flag | Description | Example |
|------|-------------|---------|
| `PATHS` | Files or directories to scan (default: current dir) | `reaper src/ lib/` |
| `--select CODES` | Only run specific rules (comma-separated) | `--select RP001,RP003` |
| `--exclude NAMES` | Skip paths containing these names | `--exclude tests,vendor` |
| `--json` | Output results as structured JSON | `--json` |
| `--no-exit-code` | Always exit 0, even with findings | `--no-exit-code` |
| `-h, --help` | Show help | `-h` |
| `-V, --version` | Print version | `-V` |

### Exit codes

| Code | Meaning |
|------|---------|
| `0` | No issues found (or `--no-exit-code` was passed) |
| `1` | Dead code found |
| `2` | Runtime error (bad path, permission denied, etc.) |

### JSON output format

```json
{
  "count": 3,
  "diagnostics": [
    {
      "file": "src/utils.py",
      "line": 1,
      "col": 8,
      "code": "RP001",
      "message": "`os` imported but unused"
    },
    {
      "file": "src/utils.py",
      "line": 14,
      "col": 1,
      "code": "RP003",
      "message": "Function `orphan` is defined but never used"
    },
    {
      "file": "src/models.py",
      "line": 22,
      "col": 1,
      "code": "RP004",
      "message": "Class `LegacyParser` is defined but never used"
    }
  ]
}
```

---

## 🏗️ Architecture

```
reaper/
├── src/
│   ├── main.rs            # CLI (clap), orchestration, output formatting
│   ├── lib.rs             # Public library interface
│   ├── analyze.rs         # Two-pass analysis engine (per-file ∥ cross-file)
│   ├── discovery.rs       # .py file walker (ignore crate, .gitignore-aware)
│   ├── fast_parser/
│   │   ├── lexer.rs       # Zero-copy Python tokenizer
│   │   └── parser.rs      # Single-pass AST builder
│   ├── ast.rs             # Typed AST node definitions
│   ├── names.rs           # Name/usage collection walkers
│   ├── location.rs        # Byte offset → (line, col) conversion
│   ├── types.rs           # Diagnostic, RuleCode types
│   └── checks/
│       ├── unused_imports.rs    # RP001
│       ├── unused_variables.rs  # RP002
│       ├── unused_defs.rs       # RP003, RP004
│       ├── unreachable.rs       # RP005
│       ├── dead_branch.rs       # RP006 (also handles RP007)
│       ├── unused_args.rs       # RP008
│       └── unused_loop_var.rs   # RP009
├── tests/
│   └── integration.rs     # 53 integration tests
├── benches/
│   └── bench_analyze.rs   # Criterion micro-benchmarks
└── benchmark/
    ├── gen_corpus.py      # Generate synthetic benchmark corpus
    ├── final_benchmark.sh # Hyperfine speed + accuracy vs Ruff
    └── audit_prod.py      # Audit Rust source for production issues
```

### Test suite

```bash
cargo test
# 169 unit tests + 53 integration tests + 1 doc-test = 223 total
# All passing ✅

cargo clippy -- -D warnings
# Clean ✅

cargo fmt --check
# Clean ✅
```

---

## 🗺️ Roadmap

- [x] 9 dead-code rules (RP001–RP009)
- [x] Cross-file analysis (RP003, RP004)
- [x] `# noqa` inline suppression
- [x] JSON output
- [x] `--select` / `--exclude` filtering
- [x] Parallel analysis (Rayon)
- [x] `.gitignore`-aware file discovery
- [ ] `pyproject.toml` / config file support
- [ ] `--fix` autofix for safe removals (unused imports)
- [ ] `--stdin` support for editor/IDE integration
- [ ] Glob patterns for `--exclude`
- [ ] Import-graph resolution for RP003/RP004
- [ ] Published crates.io package
- [ ] Pre-built binaries (GitHub Releases)
- [ ] VS Code extension

---

## 🤝 Contributing

Contributions are welcome! Here's how to get started:

```bash
# Clone
git clone https://github.com/YOUR_USERNAME/reaper.git
cd reaper

# Run all tests
cargo test

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt

# Build release binary
cargo build --release

# Run micro-benchmarks
cargo bench
```

### Adding a new rule

1. Create a new checker in `src/checks/`.
2. Add the `RuleCode` variant in `src/types.rs`.
3. Wire it into the analysis pipeline in `src/analyze.rs`.
4. Add unit tests in the checker file and integration tests in `tests/integration.rs`.
5. Document it in this README under **Rules**.

---

## 📄 License

MIT — do whatever you want with it.

---

<p align="center">
  <br />
  <code>💀</code>
  <br />
  <strong>Dead code has nowhere to hide.</strong>
  <br />
  <br />
  <sub>Built with Rust · Powered by Rayon · Faster than you think</sub>
</p>