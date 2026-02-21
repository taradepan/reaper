//! Animated welcome screen shown when `reaper` is invoked with no arguments.

use colored::Colorize;
use std::io::{self, IsTerminal, Write};
use std::thread;
use std::time::Duration;

// ── ASCII logo (REAPER in box-drawing block font) ─────────────────────────────

const LOGO: &[&str] = &[
    " ██████╗ ███████╗ █████╗ ██████╗ ███████╗██████╗ ",
    " ██╔══██╗██╔════╝██╔══██╗██╔══██╗██╔════╝██╔══██╗",
    " ██████╔╝█████╗  ███████║██████╔╝█████╗  ██████╔╝",
    " ██╔══██╗██╔══╝  ██╔══██║██╔═══╝ ██╔══╝  ██╔══██╗",
    " ██║  ██║███████╗██║  ██║██║     ███████╗██║  ██║",
    " ╚═╝  ╚═╝╚══════╝╚═╝  ╚═╝╚═╝     ╚══════╝╚═╝  ╚═╝",
];

// ── Rule catalogue ────────────────────────────────────────────────────────────

const RULES: &[(&str, &str, &str)] = &[
    ("RP001", "Unused import", "import os  # never referenced"),
    ("RP002", "Unused variable", "x = 42  # assigned, never read"),
    (
        "RP003",
        "Unused function",
        "def helper(): ...  # never called",
    ),
    (
        "RP004",
        "Unused class",
        "class Tmp: ...  # never instantiated",
    ),
    (
        "RP005",
        "Unreachable code",
        "return 1; do_thing()  # dead stmt",
    ),
    ("RP006", "Dead branch", "if True: ...  # else is dead"),
    (
        "RP007",
        "Redefined before use",
        "x = 1; x = 2  # first write lost",
    ),
    (
        "RP008",
        "Unused argument",
        "def f(x, y): return x  # y unused",
    ),
    ("RP009", "Unused loop variable", "for _ in items: pass"),
];

// ── Helpers ───────────────────────────────────────────────────────────────────

#[inline]
fn sleep(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

#[inline]
fn flush() {
    let _ = io::stdout().flush();
}

#[inline]
fn hide_cursor() {
    print!("\x1b[?25l");
    flush();
}

#[inline]
fn show_cursor() {
    print!("\x1b[?25h");
    flush();
}

/// Print without a trailing newline and flush immediately.
macro_rules! pf {
    ($($arg:tt)*) => {{
        print!($($arg)*);
        flush();
    }};
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Display the welcome screen.  Animates when stdout is a TTY; falls back to a
/// plain static print otherwise (e.g. piped output, CI, `--no-color` envs).
pub fn show_welcome() {
    if io::stdout().is_terminal() {
        // Restore cursor if we panic mid-animation.
        let _ = std::panic::catch_unwind(animated_welcome);
        show_cursor();
    } else {
        static_welcome();
    }
}

// ── Animated path (TTY) ───────────────────────────────────────────────────────

fn animated_welcome() {
    hide_cursor();

    // ── spinner intro ─────────────────────────────────────────────────────────
    let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    for (i, frame) in frames.iter().enumerate() {
        pf!(
            "\r  {}  {}",
            frame.cyan().bold(),
            "Initializing reaper…".truecolor(120, 120, 120)
        );
        // First few frames slower for dramatic effect, then speed up.
        sleep(if i < 3 { 90 } else { 55 });
    }
    pf!("\r{}\r", " ".repeat(60));

    println!();

    // ── logo lines (revealed top-to-bottom) ───────────────────────────────────
    for (i, line) in LOGO.iter().enumerate() {
        // Gradient: brighter red toward the middle rows.
        let coloured = match i {
            0 | 5 => line.truecolor(160, 20, 20).bold(),
            1 | 4 => line.truecolor(200, 30, 30).bold(),
            _ => line.truecolor(220, 50, 50).bold(),
        };
        println!("  {coloured}");
        sleep(35);
    }

    println!();

    // ── tagline (character-by-character typing effect) ────────────────────────
    let version = env!("CARGO_PKG_VERSION");
    let tagline = format!("💀  Fast Python dead-code finder  —  v{version}");

    pf!("  ");
    for ch in tagline.chars() {
        pf!("{}", ch.to_string().white().bold());
        sleep(15);
    }
    println!();
    println!();

    // ── horizontal divider ────────────────────────────────────────────────────
    let rule = "─".repeat(70);
    println!("  {}", rule.truecolor(60, 60, 60));
    println!();
    sleep(60);

    // ── rules ─────────────────────────────────────────────────────────────────
    println!("  {}", "Rules".bold().underline());
    println!();

    for (code, name, example) in RULES {
        pf!(
            "    {} ",
            code.to_string().on_truecolor(40, 40, 40).cyan().bold()
        );
        pf!("  {:<32}", name.white().bold());
        pf!("  {}", format!("# {example}").truecolor(90, 90, 90));
        println!();
        sleep(50);
    }

    println!();

    // ── divider ───────────────────────────────────────────────────────────────
    println!("  {}", rule.truecolor(60, 60, 60));
    println!();
    sleep(40);

    // ── usage ─────────────────────────────────────────────────────────────────
    println!("  {}", "Usage".bold().underline());
    println!();

    let cmds: &[(&str, &str)] = &[
        ("reaper .", "scan the current directory"),
        ("reaper src/ lib/", "scan specific paths"),
        (
            "reaper --select RP001,RP003",
            "only unused imports & functions",
        ),
        ("reaper --exclude tests,vendor", "skip directories by name"),
        ("reaper --json", "emit structured JSON output"),
        ("reaper --no-exit-code", "always exit 0  (useful in CI)"),
    ];

    for (cmd, desc) in cmds {
        println!(
            "    {}  {}",
            format!("{cmd:<40}").green().bold(),
            desc.truecolor(120, 120, 120),
        );
        sleep(35);
    }

    println!();

    // ── closing divider ───────────────────────────────────────────────────────
    println!("  {}", rule.truecolor(60, 60, 60));
    println!();

    show_cursor();
}

// ── Static / non-TTY path ─────────────────────────────────────────────────────

fn static_welcome() {
    let version = env!("CARGO_PKG_VERSION");

    for line in LOGO {
        println!("  {line}");
    }

    println!();
    println!("  Reaper v{version}  —  Fast Python dead-code finder");
    println!();
    println!("  Rules:");
    for (code, name, _example) in RULES {
        println!("    {code}  {name}");
    }
    println!();
    println!("  Usage:  reaper [PATH …] [OPTIONS]");
    println!("          reaper .                          scan current directory");
    println!("          reaper --select RP001,RP003       filter by rule");
    println!("          reaper --json                     JSON output");
    println!("          reaper --help                     full help text");
    println!();
}
