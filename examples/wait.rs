//! A wait with a countdown, a checklist and keys:
//!
//!     cargo run -p cli-kit --features tui --example wait
//!
//! `k` finishes the active step, Esc (or Ctrl‑C) leaves, `x` cancels.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cli_kit::{CliError, Flow, Mark, Tick, WaitView};

fn main() -> Result<(), CliError> {
    let flow = Flow::begin("wait demo", true);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let names = ["code accepted", "symbols compared", "approval", "store pulled"];
    let mut at = 1;
    let mut w = flow
        .wait()
        .key('k', "finish this step")
        .key('x', "cancel")
        .detach("leave");
    loop {
        let steps = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let mark = match i.cmp(&at) {
                    std::cmp::Ordering::Less => Mark::Done,
                    std::cmp::Ordering::Equal => Mark::Active,
                    std::cmp::Ordering::Greater => Mark::Pending,
                };
                (mark, n.to_string())
            })
            .collect();
        let view = WaitView {
            status: format!("Waiting for {}…", names[at.min(names.len() - 1)]),
            steps,
            deadline: Some(now + 600),
        };
        match w.tick(&view, Duration::from_millis(120))? {
            Tick::Key('k') => {
                at += 1;
                if at == names.len() {
                    w.clear();
                    flow.done("all steps done");
                    return Ok(());
                }
            }
            Tick::Key(_) => {
                w.clear();
                return Err(flow.fail("cancelled"));
            }
            Tick::Detach => {
                w.clear();
                flow.done("left; it goes on without you");
                return Ok(());
            }
            Tick::Idle => {}
        }
    }
}
