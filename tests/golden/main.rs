//! Golden capture (core refactor, M0): what cli-kit draws today, as the target
//! the refactor must reproduce. See `docs/core-refactor.md` § M0.
//!
//! Two kinds of fixture, both under `tests/golden/fixtures/`:
//!
//! - **Screens** (`screens/<scene>/<variant>.txt`). Each scene runs cli-kit's
//!   public API (a `Flow`, a prompt, an error reaching `main`) in a child of
//!   this binary on a real pty, with the pty as its controlling terminal. The
//!   test drives keys into the pty and snapshots what a terminal would show
//!   (through `avt`) at each named point: the plain text, the styled runs,
//!   the cursor, then the exit code and anything written to stdout. A screen
//!   is the right contract for drawn output, because the redraw sequences
//!   that produce it are what the refactor replaces.
//! - **Strings** (`strings/<name>.txt`). Pure functions (`glyph`,
//!   `Grid::plain`, the error chain and JSON) captured byte for byte in
//!   process, with escapes written as `\e`.
//!
//! `historical/` holds what cliclack drew for passwords before M2: the masks
//! that leaked the password's length. They are a frozen record, not a
//! target: no scene produces them, nothing compares against them, and they
//! are never rewritten or removed as stale.
//!
//! Appearance only. Behaviour the refactor fixes on purpose (Ctrl-C, resize,
//! raw mode, which stream colour follows, long lines wrapping) is not pinned
//! here: those get PTY behaviour tests of their own. The capture runs under
//! `LC_ALL=C.UTF-8` with colour forced on or off, and every screen again
//! under `LANG=C`, which must draw the same file (it is compared, never
//! written): the flow's marks do not depend on the locale.
//!
//! ```text
//! cargo test -p cli-kit --features full --test golden                         # compare
//! CLI_KIT_GOLDEN_UPDATE=1 cargo test -p cli-kit --features full --test golden # rewrite
//! cargo test -p cli-kit --features full --test golden -- input                # scenes matching
//! ```

mod pty;
mod scenes;
mod screen;
mod strings;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Set in a child: the scene to run.
const SCENE_ENV: &str = "CLI_KIT_GOLDEN_SCENE";
/// Set in a child: `1` to force colour on, `0` off.
const COLOR_ENV: &str = "CLI_KIT_GOLDEN_COLOR";
const UPDATE_ENV: &str = "CLI_KIT_GOLDEN_UPDATE";

fn main() -> ExitCode {
    if let Ok(name) = std::env::var(SCENE_ENV) {
        let color = std::env::var(COLOR_ENV).as_deref() == Ok("1");
        return scenes::run_child(&name, color);
    }

    let filter = std::env::args().skip(1).find(|a| !a.starts_with('-'));
    let update = std::env::var_os(UPDATE_ENV).is_some_and(|v| !v.is_empty());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixtures");

    let mut produced = Vec::new();
    let mut errors = Vec::new();
    for (path, result) in strings::all().into_iter().chain(scenes::all()) {
        if filter.as_deref().is_some_and(|f| !path.contains(f)) {
            continue;
        }
        match result {
            Ok(text) => produced.push((path, text)),
            Err(e) => errors.push(format!("{path}: {e}")),
        }
    }

    let mut failed = 0;
    for (path, text) in &produced {
        let file = root.join(path);
        if update {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).expect("create fixture dir");
            }
            if std::fs::read_to_string(&file).ok().as_deref() != Some(text) {
                std::fs::write(&file, text).expect("write fixture");
                eprintln!("updated {path}");
            }
            continue;
        }
        match std::fs::read_to_string(&file) {
            Ok(want) if want == *text => {}
            Ok(want) => {
                failed += 1;
                eprintln!("MISMATCH {path}\n{}", first_difference(&want, text));
            }
            Err(_) => {
                failed += 1;
                eprintln!("MISSING {path} (run with {UPDATE_ENV}=1)");
            }
        }
    }

    // A fixture nothing produces any more is a target nobody checks.
    if filter.is_none() {
        let want: BTreeSet<PathBuf> = produced.iter().map(|(p, _)| root.join(p)).collect();
        for file in fixtures(&root) {
            if want.contains(&file) || file.starts_with(root.join("historical")) {
                continue;
            }
            if update {
                std::fs::remove_file(&file).expect("remove stale fixture");
                eprintln!("removed {}", file.display());
            } else {
                failed += 1;
                eprintln!("STALE {}", file.display());
            }
        }
    }

    for e in &errors {
        eprintln!("ERROR {e}");
    }
    let total = produced.len() + errors.len();
    eprintln!(
        "golden: {total} fixtures, {} failed, {} errors",
        failed,
        errors.len()
    );
    if failed == 0 && errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn fixtures(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            out.extend(fixtures(&path));
        } else if path.extension().is_some_and(|e| e == "txt") {
            out.push(path);
        }
    }
    out
}

/// The first line that differs, with both versions, so a failure says what
/// moved without a diff tool.
fn first_difference(want: &str, got: &str) -> String {
    let (w, g): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    let n = w.len().max(g.len());
    for i in 0..n {
        let (a, b) = (w.get(i).copied(), g.get(i).copied());
        if a != b {
            return format!(
                "  line {}:\n  want: {}\n  got:  {}",
                i + 1,
                a.unwrap_or("<end>"),
                b.unwrap_or("<end>")
            );
        }
    }
    "  (only trailing newlines differ)".into()
}
