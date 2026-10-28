//! The scenes: cli-kit's public API run the way apps run it, and the keys a
//! user would press. Each runs at every listed width, with colour on and off.

use std::fmt::Debug;
use std::io::Write;
use std::process::ExitCode;

use cli_kit::flow::{self, Outcome};
use cli_kit::{CheckRow, CheckStatus, ChecksReport, CliError, Flow, Grid, Gutter, Tone};
use cli_kit::{prompts, style, ui};

use crate::pty::Session;
use crate::screen;

/// Rows on the pty: more than any scene draws, so nothing scrolls away.
const ROWS: u16 = 60;

/// One thing the driver does.
#[derive(Clone, Copy)]
enum Step {
    /// Wait until the screen shows this text, then until it settles.
    Expect(&'static str),
    /// Wait until the screen settles.
    Settle,
    /// Type these bytes.
    Keys(&'static [u8]),
    /// Record the screen under this name.
    Snap(&'static str),
    /// Make the window this many columns wide.
    Resize(u16),
}
use Step::*;

struct Scene {
    name: &'static str,
    about: &'static str,
    widths: &'static [u16],
    /// A record of a known leak, not a target (`historical/`).
    historical: bool,
    /// `TERM` for the child.
    term: &'static str,
    run: fn() -> i32,
    script: &'static [Step],
}

const XTERM: &str = "xterm-256color";

const NARROW: &[u16] = &[80, 48];
const WIDE: &[u16] = &[80];

/// The capture of every scene, by fixture path.
pub fn all() -> Vec<(String, Result<String, String>)> {
    let mut out = Vec::new();
    for scene in SCENES {
        for &cols in scene.widths {
            for color in [true, false] {
                let dir = if scene.historical {
                    "historical"
                } else {
                    "screens"
                };
                let variant = format!("w{cols}-{}", if color { "color" } else { "plain" });
                let path = format!("{dir}/{}/{variant}.txt", scene.name);
                out.push((path, capture(scene, cols, color)));
            }
        }
    }
    out
}

fn capture(scene: &Scene, cols: u16, color: bool) -> Result<String, String> {
    let mut s = Session::start(scene.name, color, cols, ROWS, scene.term)?;
    let mut out = format!(
        "# {} · {cols}×{ROWS} · {}{}\n# {}\n",
        scene.name,
        if color { "colour" } else { "no colour" },
        if scene.term == XTERM {
            String::new()
        } else {
            format!(" · TERM={}", scene.term)
        },
        scene.about
    );
    for step in scene.script {
        match *step {
            Expect(text) => s.expect(text)?,
            Settle => s.settle(),
            Keys(keys) => s.keys(keys)?,
            Resize(cols) => {
                s.resize(cols)?;
                // The prompt checks the width between keys.
                std::thread::sleep(std::time::Duration::from_millis(400));
                s.settle();
            }
            Snap(name) => {
                out += &format!("\n── {name} ──\n");
                out += &screen::snapshot(s.term(), color);
            }
        }
    }
    let (code, stdout) = s.finish()?;
    out += "\n── end ──\n";
    out += &screen::snapshot(s.term(), color);
    out += &format!("exit {code}\n");
    if !stdout.is_empty() {
        out += "stdout:\n";
        out += &stdout;
        if !stdout.ends_with('\n') {
            out.push('\n');
        }
    }
    Ok(out)
}

/// In the child: run the scene and exit with its code.
pub fn run_child(name: &str, color: bool) -> ExitCode {
    console::set_colors_enabled(color);
    console::set_colors_enabled_stderr(color);
    let Some(scene) = SCENES.iter().find(|s| s.name == name) else {
        eprintln!("no scene {name}");
        return ExitCode::from(99);
    };
    let code = (scene.run)();
    let _ = std::io::stdout().flush();
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// A prompt inside an open group, the way a command asks: what it returned
/// goes to stdout; an error reaches `main` and closes the group.
fn asked<T: Debug>(ask: impl FnOnce() -> Result<T, CliError>) -> i32 {
    let f = Flow::begin("Setup", true);
    match ask() {
        Ok(value) => {
            println!("{value:?}");
            f.done("Saved");
            0
        }
        Err(e) => {
            println!("Err({})", e.kind());
            let _ = flow::render_error(&e);
            i32::from(e.code())
        }
    }
}

const LONG: &str = "Resolved the dependency graph for every workspace member, \
                    including the optional features each app enables, in one pass";

fn sample_grid() -> Grid {
    let mut g = Grid::new(&["Name", "Kind", "Size"])
        .tones([Tone::Strong, Tone::Accent, Tone::Muted])
        .right(&[2]);
    g.lead("3 items");
    g.row(["alpha", "file", "12 KB"]);
    g.row(["bravo (two\nlines)", "dir", "—"]);
    g.row(["日本語のファイル", "file", "1.2 MB"]);
    g.row([
        style("styled").red().to_string(),
        "link".into(),
        "0 B".into(),
    ]);
    g
}

static SCENES: &[Scene] = &[
    // ── The live flow ──────────────────────────────────────────────────
    Scene {
        name: "flow_lines",
        about: "every gutter line of a live group, one wrapped and one multi-line, closed ok",
        widths: NARROW,
        historical: false,
        term: XTERM,
        run: || {
            let f = Flow::begin("Deploy", true);
            f.step("Built 3 crates");
            f.info("Using profile release");
            f.remark("cached: 12 of 14");
            f.warn("2 tests skipped");
            f.error("lint failed in cli-kit");
            f.step(LONG);
            f.info("first line\nsecond line");
            f.done("Deployed");
            0
        },
        script: &[],
    },
    Scene {
        name: "flow_blocks",
        about: "a block per gutter mark, with and without a body, a note, closed with a warning",
        widths: NARROW,
        historical: false,
        term: XTERM,
        run: || {
            let f = Flow::begin("Review", true);
            f.block(
                Gutter::Step,
                "Settled",
                "the body of a step\nover two lines",
            );
            f.block(Gutter::Info, "Context", "worth reading");
            f.block(Gutter::Warn, "Careful", "does not stop anything");
            f.block(Gutter::Error, "Blocked", "what stops the command");
            f.block(Gutter::Step, "Title only", "");
            f.note("Summary", &format!("two changes\n{LONG}"));
            f.warn_done("Reviewed with warnings");
            0
        },
        script: &[],
    },
    Scene {
        name: "flow_nested",
        about: "groups inside a group: closed ok, warn and fail, then the outer one",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || {
            let outer = Flow::begin("Release", true);
            outer.step("Version bumped");
            let a = Flow::begin("Publish", true);
            a.step("Uploaded");
            a.done("Published");
            let b = Flow::begin("Tag", true);
            b.warn_done("Tag already exists");
            let c = Flow::begin("Announce", true);
            c.end(Outcome::Fail, "Webhook unreachable");
            outer.done("Released");
            0
        },
        script: &[],
    },
    Scene {
        name: "flow_fail",
        about: "a group closed by Flow::fail, the error then reaching main",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || {
            let f = Flow::begin("Upload", true);
            f.step("Connected");
            let e = f.fail("Upload rejected: quota exceeded");
            let _ = flow::render_error(&e);
            i32::from(e.code())
        },
        script: &[],
    },
    Scene {
        name: "flow_error",
        about: "an internal error with context, and a multi-line one, reaching main inside a group",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || {
            let f = Flow::begin("Sync", true);
            f.step("Fetched");
            let io = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied");
            let e = CliError::Internal(anyhow::Error::new(io).context("writing the index"));
            let _ = flow::render_error(&e);
            let g = Flow::begin("Push", true);
            g.step("Packed");
            let e2 = CliError::External("server said:\nquota exceeded".into());
            let _ = flow::render_error(&e2);
            i32::from(e.code())
        },
        script: &[],
    },
    Scene {
        name: "flow_cancel",
        about: "Cancelled reaching main inside a group",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || {
            let f = Flow::begin("Setup", true);
            f.step("Started");
            let e = CliError::Cancelled;
            let _ = flow::render_error(&e);
            i32::from(e.code())
        },
        script: &[],
    },
    Scene {
        name: "flow_table",
        about: "a grid in its box (borderless, fitted), and a checks report",
        widths: NARROW,
        historical: false,
        term: XTERM,
        run: || {
            let f = Flow::begin("Inventory", true);
            f.table("Items", &sample_grid());
            let report = ChecksReport(vec![
                CheckRow {
                    status: CheckStatus::Pass,
                    name: "config".into(),
                    detail: "found".into(),
                },
                CheckRow {
                    status: CheckStatus::Warn,
                    name: "cache".into(),
                    detail: "stale".into(),
                },
                CheckRow {
                    status: CheckStatus::Fail,
                    name: "network".into(),
                    detail: "unreachable".into(),
                },
            ]);
            let outcome = f.checks("Doctor", &report);
            f.end(outcome, "Checked");
            0
        },
        script: &[],
    },
    Scene {
        name: "flow_plain",
        about: "the same calls on a plain flow: data on stdout, chatter on stderr",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || {
            let f = Flow::plain();
            f.step("Built 3 crates");
            f.info("Using profile release");
            f.remark("cached");
            f.warn("2 tests skipped");
            f.error("lint failed");
            f.note("Summary", "two changes");
            f.block(Gutter::Warn, "Careful", "does not stop anything");
            f.table("Items", &sample_grid());
            f.done("Deployed");
            0
        },
        script: &[],
    },
    Scene {
        name: "ui_direct",
        about: "ui's intro, outros, note and log lines called directly",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || {
            let _ = ui::intro("Direct");
            let _ = ui::log::step("step");
            let _ = ui::log::info("info");
            let _ = ui::log::remark("remark");
            let _ = ui::log::success("success");
            let _ = ui::log::warning("warning");
            let _ = ui::log::error("error");
            let _ = ui::note("A note", "its body\nover two lines");
            let _ = ui::outro("Neutral outro");
            let _ = ui::intro("Second");
            let _ = ui::outro_success("Success outro");
            let _ = ui::intro("Third");
            let _ = ui::outro_warning("Warning outro");
            let _ = ui::intro("Fourth");
            let _ = ui::outro_error("Error outro");
            0
        },
        script: &[],
    },
    Scene {
        name: "errors_plain",
        about: "each error kind reaching main with no group open",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || {
            let io = std::io::Error::other("disk full");
            for e in [
                CliError::Usage("`app add` needs a name — pass it as an argument".into()),
                CliError::needs("app add", "item", "pass it as an argument"),
                CliError::Auth("token expired".into()),
                CliError::Config("missing [server] url".into()),
                CliError::External("503 Service Unavailable".into()),
                CliError::Internal(anyhow::Error::new(io).context("saving the vault")),
                CliError::Rendered("already said".into()),
                CliError::Cancelled,
                CliError::Config("bad value".into())
                    .with_details(serde_json::json!({"key": "url"})),
            ] {
                let _ = flow::render_error(&e);
            }
            0
        },
        script: &[],
    },
    // ── The core's blocks ──────────────────────────────────────────────
    Scene {
        name: "region",
        about: "blocks replaced in place and erased on Tty::stderr(): the line above survives",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || {
            use cli_kit::render::{Block, Park, Region};
            use std::io::Write as _;
            let mut tty = cli_kit::term::Tty::stderr().expect("stderr");
            let _ = writeln!(tty, "above");
            let mut r = Region::new(tty.caps());
            let lines = |ls: &[&str]| Block::of_lines(ls.iter().map(|l| l.to_string()));
            let _ = r.draw(&mut tty, &lines(&["one", "two", "three"]));
            let _ = r.draw(
                &mut tty,
                &lines(&["parked", "", "end"]).parked(Park::At { up: 2, col: 3 }),
            );
            let _ = r.draw(&mut tty, &lines(&["a", "b", "c", "d"]));
            let _ = r.erase(&mut tty);
            let _ = r.draw(&mut tty, &lines(&["kept"]));
            let _ = r.keep(&mut tty);
            let _ = r.draw(&mut tty, &lines(&["last"]));
            0
        },
        script: &[],
    },
    // ── Prompts ────────────────────────────────────────────────────────
    Scene {
        name: "input",
        about: "prompts::input with a placeholder: active, typed, submitted",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::input("Project name", "my-project")),
        script: &[
            Expect("Project name"),
            Snap("active"),
            Keys(b"alice"),
            Expect("alice"),
            Snap("typed"),
            Keys(b"\r"),
        ],
    },
    Scene {
        name: "input_resize",
        about: "a long answer narrowed from 80 to 40 columns: repainted, the cursor in view (avt keeps the cursor row when reflowing, so ┌ scrolls into history)",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::input("Project name", "")),
        script: &[
            Expect("Project name"),
            Keys(b"a-long-project-name-that-takes-most-of-the-terminal-width-here"),
            Expect("width-here"),
            Snap("wide"),
            Resize(40),
            Snap("narrowed"),
            Keys(b"\r"),
        ],
    },
    Scene {
        name: "input_dumb",
        about: "prompts on TERM=dumb: plain steps, the terminal's own line editing and echo",
        widths: WIDE,
        historical: false,
        term: "dumb",
        run: || {
            asked(|| {
                let name = prompts::input("Project name", "")?;
                let sure = prompts::confirm("Create it?", true)?;
                Ok((name, sure))
            })
        },
        script: &[
            Expect("Project name"),
            Keys(b"\r"),
            Expect("Input required"),
            Keys(b"alicx\x7fe\r"),
            Expect("Create it?"),
            Keys(b"\r"),
        ],
    },
    Scene {
        name: "input_required",
        about: "prompts::input submitted empty: the error state, then a value",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::input("Project name", "")),
        script: &[
            Expect("Project name"),
            Keys(b"\r"),
            Expect("Input required"),
            Snap("error"),
            Keys(b"b"),
            Settle,
            Snap("typing after the error"),
            Keys(b"ob\r"),
        ],
    },
    Scene {
        name: "input_cancel",
        about: "prompts::input cancelled with Esc after typing",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::input("Project name", "my-project")),
        script: &[
            Expect("Project name"),
            Keys(b"half"),
            Expect("half"),
            Keys(b"\x1b"),
        ],
    },
    Scene {
        name: "input_default",
        about: "prompts::input_with_default: active, submitted untouched",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::input_with_default("Branch", "main")),
        script: &[Expect("Branch"), Snap("active"), Keys(b"\r")],
    },
    Scene {
        name: "input_optional",
        about: "prompts::input_optional: active, submitted empty",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::input_optional("Description", "none")),
        script: &[Expect("Description"), Snap("active"), Keys(b"\r")],
    },
    Scene {
        name: "input_optional_default",
        about: "prompts::input_optional_with_default: active, cleared, submitted",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::input_optional_with_default("Description", "kept")),
        script: &[
            Expect("Description"),
            Snap("active"),
            Keys(b"\x7f\x7f\x7f\x7f"),
            Settle,
            Snap("cleared"),
            Keys(b"\r"),
        ],
    },
    Scene {
        name: "input_multiline",
        about: "prompts::input_multiline: editing two lines, preview, submitted",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::input_multiline("Notes")),
        script: &[
            Expect("Notes"),
            Snap("active"),
            Keys(b"line one\rline two"),
            Expect("line two"),
            Snap("editing"),
            Keys(b"\x1b"),
            Settle,
            Snap("preview"),
            Keys(b"\r"),
        ],
    },
    Scene {
        name: "confirm_yes",
        about: "prompts::confirm defaulting to yes, submitted",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::confirm("Commit this?", true)),
        script: &[Expect("Commit this?"), Snap("active"), Keys(b"\r")],
    },
    Scene {
        name: "confirm_no",
        about: "prompts::confirm defaulting to no, switched with y, submitted",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::confirm("Delete everything?", false)),
        script: &[
            Expect("Delete everything?"),
            Snap("active"),
            Keys(b"y"),
            Settle,
            Snap("switched"),
            Keys(b"\r"),
        ],
    },
    Scene {
        name: "confirm_cancel",
        about: "prompts::confirm cancelled with Esc",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::confirm("Commit this?", true)),
        script: &[Expect("Commit this?"), Keys(b"\x1b")],
    },
    // ── Passwords: nothing drawn while typed ────────────────────────────
    // `historical/` keeps what cliclack drew (a mask per key, kept once
    // submitted), frozen as the record of the leak these replace.
    Scene {
        name: "password",
        about: "prompts::password: typing changes nothing on screen; submitted with no value row",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::password("Master password")),
        script: &[
            Expect("Master password"),
            Snap("active"),
            Keys(b"hunter2"),
            Settle,
            Snap("typed"),
            Keys(b"\r"),
        ],
    },
    Scene {
        name: "password_required",
        about: "prompts::password submitted empty: the step's error state, then a value",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::password("Master password")),
        script: &[
            Expect("Master password"),
            Keys(b"\r"),
            Expect("Input required"),
            Snap("error"),
            Keys(b"pw\r"),
        ],
    },
    Scene {
        name: "password_cancel",
        about: "prompts::password cancelled with Esc after typing: no value row",
        widths: WIDE,
        historical: false,
        term: XTERM,
        run: || asked(|| prompts::password("Master password")),
        script: &[
            Expect("Master password"),
            Keys(b"abc"),
            Settle,
            Keys(b"\x1b"),
        ],
    },
];
