//! Shared interactive prompt helpers built on [`cliclack`].
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

use crate::error::{CliError, map_prompt_err};

/// Run one prompt with SIGINT ignored. console's key reader answers Ctrl-C by
/// raising SIGINT at the process, which kills it mid-frame with the cursor
/// still hidden; ignored, the read fails as interrupted instead, the prompt
/// draws itself cancelled, and the caller gets [`CliError::Cancelled`]
/// (exit 130). Whatever handler was there before is put back.
fn quiet_ctrl_c<T>(prompt: impl FnOnce() -> io::Result<T>) -> Result<T, CliError> {
    #[cfg(unix)]
    let previous = unsafe {
        let mut ignore: libc::sigaction = std::mem::zeroed();
        ignore.sa_sigaction = libc::SIG_IGN;
        let mut previous: libc::sigaction = std::mem::zeroed();
        libc::sigaction(libc::SIGINT, &ignore, &mut previous);
        previous
    };
    let result = prompt();
    #[cfg(unix)]
    unsafe {
        libc::sigaction(libc::SIGINT, &previous, std::ptr::null_mut());
    }
    result.map_err(map_prompt_err)
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
    let mut p = cliclack::input(prompt);
    if !placeholder.is_empty() {
        p = p.placeholder(placeholder);
    }
    quiet_ctrl_c(|| p.interact())
}

/// Prompt for a single line of text, pre-filling a default value.
///
/// If the user presses Enter without typing, `default` is returned.
pub fn input_with_default(prompt: &str, default: &str) -> Result<String, CliError> {
    quiet_ctrl_c(|| cliclack::input(prompt).default_input(default).interact())
}

/// Prompt for a single line of text that is allowed to be empty.
///
/// Unlike `input`, this does **not** add a required-field validator.
pub fn input_optional(prompt: &str, placeholder: &str) -> Result<String, CliError> {
    let mut p = cliclack::input(prompt).required(false);
    if !placeholder.is_empty() {
        p = p.placeholder(placeholder);
    }
    quiet_ctrl_c(|| p.interact())
}

/// Prompt for optional text, pre-filling an existing value.
///
/// If the user clears the field and presses Enter the existing value is kept.
pub fn input_optional_with_default(prompt: &str, default: &str) -> Result<String, CliError> {
    quiet_ctrl_c(|| {
        cliclack::input(prompt)
            .required(false)
            .default_input(default)
            .interact()
    })
}

/// Prompt for optional multi-line text (Esc to review, Enter to submit).
pub fn input_multiline(prompt: &str) -> Result<String, CliError> {
    quiet_ctrl_c(|| {
        cliclack::input(prompt)
            .required(false)
            .multiline()
            .interact()
    })
}

// ---------------------------------------------------------------------------
// Password / hidden input
// ---------------------------------------------------------------------------

/// Prompt for a passphrase or password (input is hidden).
pub fn password(prompt: &str) -> Result<String, CliError> {
    quiet_ctrl_c(|| cliclack::password(prompt).interact())
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
    quiet_ctrl_c(|| cliclack::confirm(prompt).initial_value(default).interact())
}

// ---------------------------------------------------------------------------
// Select (single choice)
// ---------------------------------------------------------------------------

/// Prompt the user to pick **one** option from a list.
///
/// Each item is `(value, label, hint)`. Returns the chosen value.
/// Returns `Err(CliError::Cancelled)` if the user cancels.
pub fn select<T: Clone + Eq>(prompt: &str, items: &[(T, &str, &str)]) -> Result<T, CliError> {
    let mut s = cliclack::select(prompt);
    for (value, label, hint) in items {
        s = s.item(value.clone(), label, hint);
    }
    quiet_ctrl_c(|| s.interact())
}

/// [`select`] for a list that may be long: typing narrows it, at most `rows`
/// entries show at once (the rest scroll), and the foot counts them —
/// `1–8 of 120`, `5 of 120 match`. Every row shows its hint.
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
    let chosen = quiet_ctrl_c(|| crate::picker::run(list))?;
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
/// filtered by typing, counted in the foot ([`select_filtered`]). [`pick`] is the default; build one to change how many
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
pub fn multiselect<T: Clone + Eq>(
    prompt: &str,
    items: &[(T, &str, &str)],
) -> Result<Vec<T>, CliError> {
    let all_values: Vec<T> = items.iter().map(|(v, _, _)| v.clone()).collect();
    let mut s = cliclack::multiselect(prompt)
        .required(false)
        .initial_values(all_values);
    for (value, label, hint) in items {
        s = s.item(value.clone(), label, hint);
    }
    quiet_ctrl_c(|| s.interact())
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
        let during = quiet_ctrl_c(|| Ok(sigint_handler())).unwrap();
        assert_eq!(during, libc::SIG_IGN);
        assert_eq!(sigint_handler(), before, "the old handler is back");
        let err = quiet_ctrl_c::<()>(|| Err(std::io::ErrorKind::Interrupted.into()));
        assert!(matches!(err, Err(crate::CliError::Cancelled)));
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
