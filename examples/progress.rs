//! A progress bar and a live panel in one flow.
//!
//! ```text
//! cargo run -p railing --features full --example progress
//! ```

use std::thread::sleep;
use std::time::Duration;

use railing::Flow;
use railing::color::{self, Stream};
use railing::flow::Outcome;
use railing::progress::{self, Part};
use railing::style::Style;

fn main() {
    railing::ui::init_colors();
    let flow = Flow::begin("example progress", true);

    // A bar on its own line, animated for us.
    let copy = flow.progress("Copying files", 120);
    for _ in 0..120 {
        sleep(Duration::from_millis(15));
        copy.inc(1);
    }
    copy.done("Copied 120 files");

    // A panel we redraw ourselves, with a bar of several parts.
    let caps = color::caps(Stream::Stderr);
    let total = 400u64;
    if let Some(mut live) = flow.live() {
        let (mut ok, mut bad) = (0u64, 0u64);
        while ok + bad < total {
            ok += 7.min(total - ok - bad);
            if (ok + bad) % 91 == 0 && ok + bad < total {
                bad += 1;
            }
            let running = 16.min(total - ok - bad);
            let parts = [
                Part::new(ok, Style::new().green()),
                Part::new(bad, Style::new().red()),
                Part::new(running, Style::new().yellow()),
            ];
            let w = live.width().saturating_sub(24).clamp(10, 40);
            live.draw(&format!(
                "batch b-1  ·  {total} items\n\nitems  {}  {}  {}\n       {}",
                progress::bar(caps, &parts, total, w),
                progress::count(ok + bad, total),
                progress::percent(ok + bad, total),
                railing::glyph::dim(caps, &format!("✓ {ok}  ✗ {bad}  ▶ {running} running")),
            ));
            sleep(Duration::from_millis(60));
        }
        live.keep();
    }
    flow.close(Outcome::Ok, "batch finished");
}
