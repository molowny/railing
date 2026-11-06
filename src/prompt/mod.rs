//! Prompts: reading an answer from a raw terminal.
//!
//! [`secret`] reads a password or passphrase. Its read loop is given a
//! [`crate::term::Raw`] and nothing else: no handle to draw with, so it
//! cannot put anything typed on the screen. The only echo there is, a mask,
//! needs a held [`crate::term::AltScreen`] ([`Echo::Mask`]), and nothing on
//! the alternate screen outlives the session.

#[cfg(unix)]
mod ask;
#[cfg(unix)]
pub mod line;
#[cfg(unix)]
mod secret;

#[cfg(unix)]
pub use ask::{Answer, Confirm, Input, Password};
#[cfg(unix)]
pub use secret::{Echo, Paint, Secret, secret};
