//! Shared interactive prompt helpers. Text and confirm prompts are the core's
//! ([`crate::prompt`]) on the terminal the command inherited, and so is the
//! password prompt; the lists (`select`, `pick`, `multiselect`) still read
//! keys through console until the core refactor's M4.
//!
//! All functions here return `Result<T, CliError>` so cancellation (Esc /
//! Ctrl+C) automatically maps to [`CliError::Cancelled`] and propagates
//! cleanly to `main`.
//!
//! # Usage
//!
//! ```ignore
//! use cli_kit::prompts;
//!
//! let path = prompts::input("Certificate PEM path", "")?;
//! let pass = prompts::password("Key passphrase (leave blank if none)")?;
//! if prompts::confirm("Are you sure?", false)? { … }
//! ```

use std::io;

use zeroize::Zeroizing;

use crate::error::{CliError, map_prompt_err};

/// Run one prompt with SIGINT ignored. console's key reader answers Ctrl-C by
/// raising SIGINT at the process, which kills it mid-frame with the cursor
/// still hidden; ignored, the read fails as interrupted instead, the prompt
/// draws itself cancelled, and the caller gets [`CliError::Cancelled`]
/// (exit 130). Whatever handler was there before is put back. A cancelled
/// prompt's footer ends the gutter, so the open groups are marked closed
/// ([`crate::flow::closed_by_prompt`]) and nothing draws a second `└`.
///
/// Off Unix there is no terminal to ask on ([`crate::term::SUPPORTED`]): the
/// prompt is a usage error naming what to pass instead, the same exit a
/// missing argument gets where it cannot be asked for.
fn quiet_ctrl_c<T>(what: &str, prompt: impl FnOnce() -> io::Result<T>) -> Result<T, CliError> {
    if !crate::term::SUPPORTED {
        return Err(CliError::Usage(format!(
            "interactive prompts need a Unix terminal; pass {} as an argument",
            what.trim_end_matches(['?', ':']).trim()
        )));
    }
    let result = without_sigint(prompt).map_err(map_prompt_err);
    if matches!(result, Err(CliError::Cancelled)) {
        crate::flow::prompt_cancelled();
    }
    result
}

/// Run `f` with SIGINT ignored, putting back whatever handler was there: a
/// key reader then sees Ctrl-C as a key or an interrupted read instead of the
/// process dying mid-frame.
pub(crate) fn without_sigint<T>(f: impl FnOnce() -> T) -> T {
    #[cfg(unix)]
    let previous = unsafe {
        let mut ignore: libc::sigaction = std::mem::zeroed();
        ignore.sa_sigaction = libc::SIG_IGN;
        let mut previous: libc::sigaction = std::mem::zeroed();
        libc::sigaction(libc::SIGINT, &ignore, &mut previous);
        previous
    };
    let result = f();
    #[cfg(unix)]
    unsafe {
        libc::sigaction(libc::SIGINT, &previous, std::ptr::null_mut());
    }
    result
}

// ---------------------------------------------------------------------------
// Asking through the core
// ---------------------------------------------------------------------------

/// Ask on the terminal this command inherited (`term::Tty::stderr`), and
/// turn how it ended into a result: Esc or Ctrl-C is [`CliError::Cancelled`]
/// with the gutter marked closed (the cancelled step's footer ends it); a
/// signal installed through `term::signals` is re-raised. Asking where there
/// is no terminal to ask on is a usage error naming what to pass instead.
///
/// Colour follows `console`'s stderr flag, like everything else the full
/// kit draws (until it holds a `Tty` of its own, core refactor M5).
#[cfg(unix)]
fn ask_on_stderr<T>(
    what: &str,
    ask: impl FnOnce(&mut crate::term::Tty) -> io::Result<crate::prompt::Answer<T>>,
) -> Result<T, CliError> {
    use crate::prompt::Answer;
    let tty = crate::term::Tty::stderr().map_err(|e| CliError::Internal(anyhow::anyhow!(e)))?;
    let caps = crate::style::Caps {
        color: crate::ui::stderr_caps().color,
        ..tty.caps()
    };
    let mut tty = tty.with_caps(caps);
    match ask(&mut tty) {
        Ok(Answer::Submitted(v)) => Ok(v),
        Ok(Answer::Cancelled) => {
            crate::flow::prompt_cancelled();
            Err(CliError::Cancelled)
        }
        Ok(Answer::Hangup) => Err(CliError::Cancelled),
        Ok(Answer::Interrupted(sig)) => crate::term::signals::reraise(sig),
        Err(e) if e.raw_os_error() == Some(libc::ENOTTY) => Err(CliError::Usage(format!(
            "interactive prompts need a terminal; pass {} as an argument",
            what_to_pass(what)
        ))),
        Err(e) => Err(CliError::Internal(anyhow::anyhow!(e))),
    }
}

/// The prompt's question as the thing to pass: `Project name?` → `Project name`.
fn what_to_pass(prompt: &str) -> &str {
    prompt.trim_end_matches(['?', ':']).trim()
}

#[cfg(not(unix))]
fn no_terminal_here(prompt: &str) -> CliError {
    CliError::Usage(format!(
        "interactive prompts need a Unix terminal; pass {} as an argument",
        what_to_pass(prompt)
    ))
}

// ---------------------------------------------------------------------------
// Text input
// ---------------------------------------------------------------------------

/// Prompt for a single line of text.
///
/// * `prompt`      — the question shown to the user.
/// * `placeholder` — hint text shown when the field is empty (pass `""` for
///   none).
pub fn input(prompt: &str, placeholder: &str) -> Result<String, CliError> {
    #[cfg(unix)]
    return ask_on_stderr(prompt, |tty| {
        crate::prompt::Input::new(prompt)
            .placeholder(placeholder)
            .ask(tty)
    });
    #[cfg(not(unix))]
    {
        let _ = placeholder;
        Err(no_terminal_here(prompt))
    }
}

/// Prompt for a single line of text, pre-filling a default value.
///
/// If the user presses Enter without typing, `default` is returned.
pub fn input_with_default(prompt: &str, default: &str) -> Result<String, CliError> {
    #[cfg(unix)]
    return ask_on_stderr(prompt, |tty| {
        crate::prompt::Input::new(prompt).default(default).ask(tty)
    });
    #[cfg(not(unix))]
    {
        let _ = default;
        Err(no_terminal_here(prompt))
    }
}

/// Prompt for a single line of text that is allowed to be empty.
///
/// Unlike `input`, this does **not** add a required-field validator.
pub fn input_optional(prompt: &str, placeholder: &str) -> Result<String, CliError> {
    #[cfg(unix)]
    return ask_on_stderr(prompt, |tty| {
        crate::prompt::Input::new(prompt)
            .placeholder(placeholder)
            .required(false)
            .ask(tty)
    });
    #[cfg(not(unix))]
    {
        let _ = placeholder;
        Err(no_terminal_here(prompt))
    }
}

/// Prompt for optional text, pre-filling an existing value.
///
/// If the user clears the field and presses Enter the existing value is kept.
pub fn input_optional_with_default(prompt: &str, default: &str) -> Result<String, CliError> {
    #[cfg(unix)]
    return ask_on_stderr(prompt, |tty| {
        crate::prompt::Input::new(prompt)
            .required(false)
            .default(default)
            .ask(tty)
    });
    #[cfg(not(unix))]
    {
        let _ = default;
        Err(no_terminal_here(prompt))
    }
}

/// Prompt for optional multi-line text (Esc to review, Enter to submit).
pub fn input_multiline(prompt: &str) -> Result<String, CliError> {
    #[cfg(unix)]
    return ask_on_stderr(prompt, |tty| {
        crate::prompt::Input::new(prompt)
            .required(false)
            .multiline()
            .ask(tty)
    });
    #[cfg(not(unix))]
    Err(no_terminal_here(prompt))
}

// ---------------------------------------------------------------------------
// Password / hidden input
// ---------------------------------------------------------------------------

/// Prompt for a passphrase or password.
///
/// Nothing is drawn while it is typed, not even a mask: the step stays as
/// drawn, then collapses to `◇  prompt` with no value row, so nothing about
/// the secret (its length included) reaches scrollback or a log of the
/// terminal. The answer is wiped when dropped: keep it in `Zeroizing` up to
/// where it is used, and make any copy out of it visible at the call site.
pub fn password(prompt: &str) -> Result<Zeroizing<String>, CliError> {
    #[cfg(unix)]
    return ask_on_stderr(prompt, |tty| crate::prompt::Password::new(prompt).ask(tty));
    #[cfg(not(unix))]
    Err(no_terminal_here(prompt))
}

// ---------------------------------------------------------------------------
// Confirm (yes / no)
// ---------------------------------------------------------------------------

/// Ask a yes/no question with an explicit pre-selected default.
///
/// Pass `true` for happy-path confirmations ("Commit this?") and `false`
/// for destructive or skippable steps ("Delete everything?").
/// Returns `Err(CliError::Cancelled)` if the user cancels.
pub fn confirm(prompt: &str, default: bool) -> Result<bool, CliError> {
    #[cfg(unix)]
    return ask_on_stderr(prompt, |tty| {
        crate::prompt::Confirm::new(prompt, default).ask(tty)
    });
    #[cfg(not(unix))]
    {
        let _ = default;
        Err(no_terminal_here(prompt))
    }
}

// ---------------------------------------------------------------------------
// Select (single choice)
// ---------------------------------------------------------------------------

/// Prompt the user to pick **one** option from a list.
///
/// Each item is `(value, label, hint)`. Returns the chosen value.
/// Returns `Err(CliError::Cancelled)` if the user cancels.
///
/// A short, fixed set of choices, read whole: no filter (that is the picker,
/// [`pick`]), letters do nothing, Esc cancels. It draws in the picker's box —
/// the `›` cursor, every hint, the controls line — sized to its rows, with a
/// count only when it has to scroll.
pub fn select<T: Clone + Eq>(prompt: &str, items: &[(T, &str, &str)]) -> Result<T, CliError> {
    let list = crate::picker::List::new(
        prompt,
        items
            .iter()
            .map(|(_, label, hint)| (*label, *hint))
            .collect(),
        Rows::Auto.resolve(),
    )
    .select();
    let chosen = quiet_ctrl_c(prompt, || crate::picker::run(list))?;
    Ok(items[chosen].0.clone())
}

/// [`select`] for a list that may be long: typing narrows it, at most `rows`
/// entries show at once (the rest scroll), in the browsed grid's box: the
/// filter and the count on its title — `1–8 of 120`, `5 of 120 match · 1–5`.
pub fn select_filtered<T: Clone>(
    prompt: &str,
    items: &[(T, &str, &str)],
    rows: usize,
) -> Result<T, CliError> {
    let list = crate::picker::List::new(
        prompt,
        items
            .iter()
            .map(|(_, label, hint)| (*label, *hint))
            .collect(),
        rows,
    );
    let chosen = quiet_ctrl_c(prompt, || crate::picker::run(list))?;
    Ok(items[chosen].0.clone())
}

/// How many rows a [`Picker`] shows at once; typing narrows the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rows {
    /// About half the terminal (see [`Rows::for_height`]): past half a
    /// screen, reading the list is slower than typing to narrow it.
    #[default]
    Auto,
    /// Exactly this many (at least one).
    Fixed(usize),
}

impl Rows {
    /// The rows to show on a terminal `height` lines tall: half of it for
    /// [`Rows::Auto`], kept between 5 and 25 so a tiny or a huge window still
    /// gets a usable list.
    pub fn for_height(self, height: u16) -> usize {
        match self {
            Rows::Fixed(n) => n.max(1),
            Rows::Auto => (usize::from(height) / 2).clamp(5, 25),
        }
    }

    fn resolve(self) -> usize {
        let (height, _) = console::Term::stderr().size();
        self.for_height(height)
    }
}

/// The picker for a left-out argument: owned `(value, label, hint)` rows,
/// filtered by typing, counted on the box title ([`select_filtered`]). [`pick`] is the default; build one to change how many
/// rows it shows:
///
/// ```ignore
/// let rev = Picker::new("Revision").rows(Rows::Fixed(5)).pick(&rows)?;
/// ```
#[derive(Debug, Clone)]
pub struct Picker {
    prompt: String,
    rows: Rows,
}

impl Picker {
    pub fn new(prompt: &str) -> Self {
        Self {
            prompt: prompt.to_string(),
            rows: Rows::default(),
        }
    }

    pub fn rows(mut self, rows: Rows) -> Self {
        self.rows = rows;
        self
    }

    /// Ask. Callers build rows from whatever they list (items, runs,
    /// profiles), so this owns nothing of theirs.
    pub fn pick<T: Clone>(&self, rows: &[(T, String, String)]) -> Result<T, CliError> {
        let items: Vec<(T, &str, &str)> = rows
            .iter()
            .map(|(v, l, h)| (v.clone(), l.as_str(), h.as_str()))
            .collect();
        select_filtered(&self.prompt, &items, self.rows.resolve())
    }
}

/// [`Picker`] with its defaults ([`Rows::Auto`]).
pub fn pick<T: Clone>(prompt: &str, rows: &[(T, String, String)]) -> Result<T, CliError> {
    Picker::new(prompt).pick(rows)
}

// ---------------------------------------------------------------------------
// Multi-select (multiple choices)
// ---------------------------------------------------------------------------

/// Prompt the user to pick **zero or more** options from a list.
///
/// Each item is `(value, label, hint)`. All items are pre-selected by default.
/// Returns `Err(CliError::Cancelled)` if the user cancels.
///
/// [`select`]'s box with a checkbox per row and the count on the title
/// (`3 of 5 selected`): `space` toggles a row, `a` all of them, Enter returns
/// the checked ones (none is a valid answer), Esc cancels.
pub fn multiselect<T: Clone + Eq>(
    prompt: &str,
    items: &[(T, &str, &str)],
) -> Result<Vec<T>, CliError> {
    let list = crate::picker::List::new(
        prompt,
        items
            .iter()
            .map(|(_, label, hint)| (*label, *hint))
            .collect(),
        Rows::Auto.resolve(),
    )
    .multi(vec![true; items.len()]);
    let chosen = quiet_ctrl_c(prompt, || crate::picker::run_multi(list))?;
    Ok(chosen.into_iter().map(|i| items[i].0.clone()).collect())
}

#[cfg(test)]
mod tests {
    use super::{Rows, quiet_ctrl_c};

    #[cfg(unix)]
    fn sigint_handler() -> libc::sighandler_t {
        unsafe {
            let mut now: libc::sigaction = std::mem::zeroed();
            libc::sigaction(libc::SIGINT, std::ptr::null(), &mut now);
            now.sa_sigaction
        }
    }

    #[cfg(unix)]
    #[test]
    fn ctrl_c_is_ignored_only_while_asking() {
        let before = sigint_handler();
        let during = quiet_ctrl_c("a value", || Ok(sigint_handler())).unwrap();
        assert_eq!(during, libc::SIG_IGN);
        assert_eq!(sigint_handler(), before, "the old handler is back");
        let err = quiet_ctrl_c::<()>("a value", || Err(std::io::ErrorKind::Interrupted.into()));
        assert!(matches!(err, Err(crate::CliError::Cancelled)));
        assert!(
            crate::flow::closed_by_prompt(),
            "the prompt's footer closed the gutter"
        );
    }

    #[test]
    fn auto_rows_are_half_the_terminal_within_bounds() {
        assert_eq!(Rows::Auto.for_height(40), 20);
        assert_eq!(Rows::Auto.for_height(24), 12);
        assert_eq!(Rows::Auto.for_height(6), 5, "a tiny window still gets 5");
        assert_eq!(Rows::Auto.for_height(200), 25, "a huge one is capped");
    }

    #[test]
    fn fixed_rows_are_taken_as_given_but_never_zero() {
        assert_eq!(Rows::Fixed(7).for_height(40), 7);
        assert_eq!(Rows::Fixed(0).for_height(40), 1);
    }
}
