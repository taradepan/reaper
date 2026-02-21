use criterion::{Criterion, black_box, criterion_group, criterion_main};
use reaper::analyze::analyze_files;
use std::fs;
use tempfile::TempDir;

/// Generate a realistic Python file with a mix of imports, functions, classes,
/// and control flow so all checkers get exercised.
fn make_python_file(index: usize) -> String {
    format!(
        r#"
import os
import sys
import re
from pathlib import Path
from collections import OrderedDict

CONSTANT_{i} = {i}

def used_function_{i}(x, y):
    result = x + y
    return result

def unused_function_{i}(a, b):
    temp = a * b
    return temp

class UsedClass_{i}:
    def __init__(self, value):
        self.value = value

    def compute(self):
        return self.value * 2

class UnusedClass_{i}:
    pass

def has_dead_code_{i}():
    if False:
        never = 1
    x = used_function_{i}(1, 2)
    return x

def has_unreachable_{i}():
    return 42
    dead = True  # noqa: RP005

instance_{i} = UsedClass_{i}(CONSTANT_{i})
result_{i} = has_dead_code_{i}()
print(os.path.join("a", "b"))
print(sys.version)
_ = re.compile(r"\d+")
p = Path(".")
d: OrderedDict = OrderedDict()
"#,
        i = index
    )
}

fn bench_analyze(c: &mut Criterion) {
    // Build a temporary corpus of 50 Python files.
    let dir = TempDir::new().unwrap();
    let mut files = Vec::new();
    for i in 0..50 {
        let path = dir.path().join(format!("module_{i}.py"));
        fs::write(&path, make_python_file(i)).unwrap();
        files.push(path);
    }

    c.bench_function("analyze_files_50_modules", |b| {
        b.iter(|| {
            let diags = analyze_files(black_box(&files)).unwrap();
            black_box(diags);
        });
    });

    // Also benchmark a single large file.
    let big_source: String = (0..200)
        .map(make_python_file)
        .collect::<Vec<_>>()
        .join("\n");
    let big_path = dir.path().join("big.py");
    fs::write(&big_path, &big_source).unwrap();

    c.bench_function("analyze_files_single_large_file", |b| {
        b.iter(|| {
            let diags = analyze_files(black_box(&[big_path.clone()])).unwrap();
            black_box(diags);
        });
    });
}

criterion_group!(benches, bench_analyze);
criterion_main!(benches);
