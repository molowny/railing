//! Argument parsing that keeps JSON mode's error contract.
//!
//! `Parser::parse` prints clap's own text and exits, so a client that asked
//! for JSON gets prose for a mistyped flag. [`parse`] reports that error as
//! the same JSON document every other error uses ([`crate::error`]).

use std::ffi::OsString;

use clap::Parser;
use clap::error::ErrorKind;

use crate::CliError;
use crate::output::{OutputFormat, set_format};

/// [`Parser::parse`], except that a parse error is printed as a JSON
/// `usage` error (exit 2) when the arguments ask for JSON ([`wants_json`]).
/// `--help` and `--version` are never errors and print as usual.
#[allow(clippy::print_stdout)]
pub fn parse<C: Parser>() -> C {
    let args: Vec<OsString> = std::env::args_os().collect();
    match C::try_parse_from(&args) {
        Ok(c) => c,
        Err(e) if is_failure(e.kind()) && wants_json(&args[1.min(args.len())..]) => {
            set_format(OutputFormat::Json);
            let _ = CliError::Usage(clap_message(&e)).render();
            std::process::exit(2);
        }
        Err(e) => e.exit(),
    }
}

fn is_failure(kind: ErrorKind) -> bool {
    !matches!(
        kind,
        ErrorKind::DisplayHelp
            | ErrorKind::DisplayVersion
            | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
    )
}

/// clap's message without its `error: ` prefix, colors or usage footer.
fn clap_message(e: &clap::Error) -> String {
    let text = crate::style::strip(&e.render().to_string()).to_string();
    let first = text
        .split("\n\n")
        .next()
        .unwrap_or_default()
        .trim()
        .trim_start_matches("error: ");
    first.to_string()
}

/// Whether the arguments ask for JSON in any of the forms the apps use:
/// `--json`, `-J` (alone or in a cluster such as `-qJ`), `--format json` and
/// `--format=json`. Anything after `--` belongs to another program.
pub fn wants_json(args: &[OsString]) -> bool {
    let mut prev_format = false;
    for a in args {
        let Some(a) = a.to_str() else {
            prev_format = false;
            continue;
        };
        if a == "--" {
            return false;
        }
        if prev_format && a == "json" {
            return true;
        }
        prev_format = a == "--format";
        if a == "--json" || a == "--format=json" {
            return true;
        }
        if let Some(cluster) = a.strip_prefix('-')
            && !cluster.starts_with('-')
            && cluster.contains('J')
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<OsString> {
        v.iter().map(OsString::from).collect()
    }

    #[test]
    fn json_forms_are_recognised() {
        for v in [
            &["get", "--json"][..],
            &["get", "-J"],
            &["get", "-qJ"],
            &["--format", "json", "list"],
            &["--format=json"],
        ] {
            assert!(wants_json(&args(v)), "{v:?}");
        }
    }

    #[test]
    fn other_args_are_not_json() {
        for v in [
            &["get", "PayPal"][..],
            &["--format", "pretty"],
            &["exec", "--", "tool", "--json"],
            &["get", "json"],
        ] {
            assert!(!wants_json(&args(v)), "{v:?}");
        }
    }

    #[derive(Parser, Debug)]
    struct Demo {
        #[arg(long)]
        name: String,
    }

    #[test]
    fn clap_message_is_the_first_paragraph() {
        let e = Demo::try_parse_from(["demo"]).unwrap_err();
        let m = clap_message(&e);
        assert!(m.contains("--name"), "{m}");
        assert!(!m.starts_with("error:") && !m.contains("Usage:"), "{m}");
    }
}
