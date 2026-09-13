//! Demonstrates the inline fuzzy picker (requires `picker` feature).
//!
//! Run with: `cargo run --example picker --features picker`

use cli_kit::picker::{pick, Badge, Color, PickerConfig, PickerItem, PickerResult};

fn item(
    id: &str,
    title: &str,
    kind: &str,
    color: Color,
    subtitle: &str,
    recent: bool,
) -> PickerItem {
    PickerItem {
        id: id.into(),
        title: title.into(),
        badge: Badge {
            text: Box::leak(kind.to_string().into_boxed_str()),
            color,
        },
        subtitle: subtitle.into(),
        match_text: format!("{title} {subtitle}"),
        is_recent: recent,
    }
}

fn main() -> std::io::Result<()> {
    if !cli_kit::picker::is_available() {
        eprintln!("no /dev/tty available — picker needs an interactive terminal");
        return Ok(());
    }

    let items = vec![
        item("1", "GitHub", "LOGIN", Color::Blue, "personal", true),
        item("2", "AWS root", "LOGIN", Color::Blue, "work", false),
        item(
            "3",
            "Recovery codes",
            "NOTE",
            Color::Yellow,
            "personal",
            false,
        ),
        item("4", "Passport scan", "FILE", Color::Magenta, "docs", false),
        item("5", "Wi-Fi home", "LOGIN", Color::Blue, "personal", true),
    ];

    let config = PickerConfig {
        prompt: "> ",
        initial_query: "",
        footer_hints: vec![("↑/↓", "navigate"), ("Enter", "select"), ("Esc", "cancel")],
    };

    match pick(&items, &config)? {
        PickerResult::Selected(idx) => println!("selected: {}", items[idx].title),
        PickerResult::Cancelled => println!("cancelled"),
    }
    Ok(())
}
