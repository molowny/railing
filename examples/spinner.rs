//! The spinner behind `Flow::task` and `ui::Spinner`: a slow step that
//! changes its message, one that ends as a `◇` line, one that is cleared.
//!
//! Run with:
//!   cargo run -p cli-kit --example spinner            # about 6 seconds
//!   cargo run -p cli-kit --example spinner -- slow    # three times as long, to watch it

use std::thread::sleep;
use std::time::Duration;

use cli_kit::Flow;

fn main() {
    cli_kit::ui::init_colors();
    let pace = if std::env::args().any(|a| a == "slow") {
        3
    } else {
        1
    };
    let wait = |ms: u64| sleep(Duration::from_millis(ms * pace));
    let live = console::user_attended_stderr();
    let flow = Flow::begin("spinner example", live);

    let task = flow.task("Fetching the item list");
    wait(1500);
    task.update("Decrypting 312 items");
    wait(1500);
    task.done("312 items loaded");

    let task = flow.task("Checking the remote");
    wait(1500);
    task.clear();

    if let Some(s) = flow.spinner("Generating commit message suggestions") {
        wait(1500);
        s.stop("3 valid suggestions");
    }

    flow.done("spinner example finished");
}
