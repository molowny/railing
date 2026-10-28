//! The core never reaches a terminal, a stream or the environment the caller
//! did not name (askpass bar, A1 and A2): no `/dev/tty`, no stdio, no
//! `std::env`, no printing, no `console`. The two exceptions are the named
//! constructors themselves: `Tty::stderr` and `TermEnv::of_process`.
//!
//! Read off the source, tests and comments left out, so a new call is caught
//! where it is written, before any app runs into it.

use std::path::{Path, PathBuf};

/// What the core must not touch.
const FORBIDDEN: &[&str] = &[
    "/dev/tty",
    "io::stderr",
    "io::stdin",
    "io::stdout",
    "std::env",
    "env::var",
    "print!",
    "println!",
    "eprint!",
    "eprintln!",
    "console::",
];

/// (file, function, what it may use): the named ways in.
const ALLOWED: &[(&str, &str, &str)] = &[
    ("term/mod.rs", "stderr", "io::stderr"),
    ("term/mod.rs", "stderr", "io::stdin"),
    ("style.rs", "of_process", "std::env"),
    ("style.rs", "of_process", "env::var"),
];

fn core_files() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<PathBuf> = ["style.rs", "glyph.rs", "spin.rs", "render.rs", "step.rs"]
        .iter()
        .map(|f| src.join(f))
        .collect();
    for dir in ["term", "prompt"] {
        for entry in std::fs::read_dir(src.join(dir)).unwrap() {
            files.push(entry.unwrap().path());
        }
    }
    files
}

/// The code of `text`, line by line: comments blanked, and the test module
/// (from its `#[cfg(...test...)]` to the end) dropped.
fn code(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("#[cfg(test)]") || t.starts_with("#[cfg(all(test") {
            break;
        }
        let code = match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        };
        out.push(code.to_string());
    }
    out
}

/// The lines of `fn name`'s body: from its signature to the brace closing it.
fn body(lines: &[String], name: &str) -> Option<std::ops::Range<usize>> {
    let start = lines
        .iter()
        .position(|l| l.contains(&format!("fn {name}(")))?;
    let mut depth = 0i32;
    let mut opened = false;
    for (i, l) in lines.iter().enumerate().skip(start) {
        for c in l.chars() {
            match c {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth == 0 {
            return Some(start..i + 1);
        }
    }
    None
}

#[test]
fn the_core_reaches_nothing_it_was_not_given() {
    let mut found = Vec::new();
    for file in core_files() {
        let rel = file
            .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let lines = code(&std::fs::read_to_string(&file).unwrap());
        for (n, line) in lines.iter().enumerate() {
            for token in FORBIDDEN {
                if !line.contains(token) {
                    continue;
                }
                let allowed = ALLOWED.iter().any(|(f, func, t)| {
                    *f == rel && t == token && body(&lines, func).is_some_and(|r| r.contains(&n))
                });
                if !allowed {
                    found.push(format!("src/{rel}:{}: {token}: {}", n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "the core reaches past its caller:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_named_ways_in_are_still_there() {
    // An allowance that no longer matches anything is one nobody checks.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for (file, func, token) in ALLOWED {
        let lines = code(&std::fs::read_to_string(src.join(file)).unwrap());
        let range = body(&lines, func).unwrap_or_else(|| panic!("no fn {func} in {file}"));
        assert!(
            lines[range].iter().any(|l| l.contains(token)),
            "fn {func} in {file} no longer uses {token}"
        );
    }
}
