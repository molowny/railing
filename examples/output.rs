//! Demonstrates the `Renderable` trait, table helpers, and `DiagnosticsOutput`.
//!
//! Run with:
//!   cargo run --example output
//!   cargo run --example output -- json

use railing::{DiagnosticsOutput, Grid, OutputFormat, Renderable};

struct User {
    name: String,
    email: String,
    admin: bool,
}

struct UserList(Vec<User>);

impl Renderable for UserList {
    fn render_pretty(&self) {
        let mut table = Grid::new(&["Name", "Email", "Role"]);
        for u in &self.0 {
            table.row([
                u.name.as_str(),
                u.email.as_str(),
                if u.admin { "admin" } else { "user" },
            ]);
        }
        println!("{}", table.plain());
    }

    fn render_json(&self) {
        let arr: Vec<_> = self
            .0
            .iter()
            .map(|u| {
                serde_json::json!({
                    "name": u.name,
                    "email": u.email,
                    "admin": u.admin,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr).unwrap());
    }
}

fn main() {
    let format = if std::env::args().any(|a| a == "json") {
        OutputFormat::Json
    } else {
        OutputFormat::Pretty
    };

    let users = UserList(vec![
        User {
            name: "Alice".into(),
            email: "alice@example.com".into(),
            admin: true,
        },
        User {
            name: "Bob".into(),
            email: "bob@example.com".into(),
            admin: false,
        },
    ]);
    users.render(format);

    println!();

    let diag = DiagnosticsOutput(vec![
        ("config_path".into(), "~/.config/app/config.toml".into()),
        ("profile".into(), "default".into()),
        ("version".into(), env!("CARGO_PKG_VERSION").into()),
    ]);
    diag.render(format);
}
