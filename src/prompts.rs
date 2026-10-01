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

use crate::error::{CliError, map_prompt_err};

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
    p.interact().map_err(map_prompt_err)
}

/// Prompt for a single line of text, pre-filling a default value.
///
/// If the user presses Enter without typing, `default` is returned.
pub fn input_with_default(prompt: &str, default: &str) -> Result<String, CliError> {
    cliclack::input(prompt)
        .default_input(default)
        .interact()
        .map_err(map_prompt_err)
}

/// Prompt for a single line of text that is allowed to be empty.
///
/// Unlike `input`, this does **not** add a required-field validator.
pub fn input_optional(prompt: &str, placeholder: &str) -> Result<String, CliError> {
    let mut p = cliclack::input(prompt).required(false);
    if !placeholder.is_empty() {
        p = p.placeholder(placeholder);
    }
    p.interact().map_err(map_prompt_err)
}

/// Prompt for optional text, pre-filling an existing value.
///
/// If the user clears the field and presses Enter the existing value is kept.
pub fn input_optional_with_default(prompt: &str, default: &str) -> Result<String, CliError> {
    cliclack::input(prompt)
        .required(false)
        .default_input(default)
        .interact()
        .map_err(map_prompt_err)
}

/// Prompt for optional multi-line text (Esc to review, Enter to submit).
pub fn input_multiline(prompt: &str) -> Result<String, CliError> {
    cliclack::input(prompt)
        .required(false)
        .multiline()
        .interact()
        .map_err(map_prompt_err)
}

// ---------------------------------------------------------------------------
// Password / hidden input
// ---------------------------------------------------------------------------

/// Prompt for a passphrase or password (input is hidden).
pub fn password(prompt: &str) -> Result<String, CliError> {
    cliclack::password(prompt)
        .interact()
        .map_err(map_prompt_err)
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
    cliclack::confirm(prompt)
        .initial_value(default)
        .interact()
        .map_err(map_prompt_err)
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
    s.interact().map_err(map_prompt_err)
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
    s.interact().map_err(map_prompt_err)
}
