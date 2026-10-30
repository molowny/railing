//! The spinner behind `Flow::task` and `ui::Spinner`, in every style it
//! offers: for each, a slow step that changes its message and ends as a `◇`
//! line; then one task that is cleared and one bare spinner.
//!
//! Run with:
//!   cargo run -p cli-kit --example spinner                  # every style, one after another
//!   cargo run -p cli-kit --example spinner -- slow          # three times as long, to watch it
//!   cargo run -p cli-kit --example spinner -- square slow   # one style: star, diamond, square, circle

use std::thread::sleep;
use std::time::Duration;

use cli_kit::Flow;
use cli_kit::ui::{SpinnerStyle, set_spinner_style};

fn main() {
    cli_kit::ui::init_colors();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pace = if args.iter().any(|a| a == "slow") {
        3
    } else {
        1
    };
    let wait = |ms: u64| sleep(Duration::from_millis(ms * pace));
    let chosen: Vec<SpinnerStyle> = args.iter().filter_map(|a| a.parse().ok()).collect();
    let styles = if chosen.is_empty() {
        SpinnerStyle::ALL.to_vec()
    } else {
        chosen
    };

    let live = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let flow = Flow::begin("spinner example", live);

    for &style in &styles {
        // Each spinner takes the program's style when it starts.
        set_spinner_style(style);
        let name = style.name();
        let task = flow.task(&format!("{name}: fetching the item list"));
        wait(1200);
        task.update(&format!("{name}: decrypting 312 items"));
        wait(1200);
        task.done(&format!("{name}: 312 items loaded"));
    }

    let task = flow.task("Checking the remote (cleared: leaves nothing)");
    wait(1200);
    task.clear();

    if let Some(s) = flow.spinner("Generating commit message suggestions") {
        wait(1200);
        s.stop("3 valid suggestions");
    }

    flow.done(&format!("{} style(s) shown", styles.len()));
}
