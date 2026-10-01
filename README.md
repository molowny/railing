# cli-kit

Reusable CLI helpers library for Rust command-line applications. Provides structured output, interactive prompts, logging bootstrap, and optional TUI components.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
cli-kit = "0.1"
```

To enable optional features:

```toml
[dependencies]
cli-kit = { version = "0.1", features = ["picker"] }
```

## Modules

### Core

| Module | Description |
|--------|-------------|
| `flow` | One command as one clack prompt group (`┌ … ◇ … └ ✔`): `Flow` (live or plain), `Grid` tables boxed inside steps, checks, error close via `render_error` |
| `args` | `args::parse::<Cli>()`: clap parsing whose errors follow the JSON error contract when the arguments ask for JSON |
| `error` | Categorized `CliError` enum (Usage, Cancelled, Auth, Config, External, Internal) with styled rendering, proper exit codes, and a JSON form (`kind`, `to_json`, `with_details`) |
| `logging` | Tracing subscriber bootstrap with `-v`/`-vv` verbosity levels |
| `output` | `Renderable` trait for pretty/JSON dual rendering, table helpers, `GlobalOpts`, `set_format` (opt into JSON errors) |
| `prompts` | Text input, password, and confirmation prompts with automatic cancellation handling |
| `ui` | Colored alerts, spinners, intro/outro banners |

### Optional

| Module | Feature flag | Description |
|--------|-------------|-------------|
| `tui` | `tui` | Ratatui-based component model with flex layout, paint utilities, theme constants, widgets, and the [`screen`](#tui-surfaces) app runner |
| `picker` | `picker` | Inline fuzzy picker with match highlighting, adaptive sizing, and score-based sorting |

## Feature flags

| Flag | Enables | Extra dependencies |
|------|---------|-------------------|
| `tui` | TUI components and layout system | `ratatui`, `unicode-width` |
| `picker` | Interactive fuzzy picker (implies `tui`) | `nucleo-matcher`, `crossterm` |

## Examples

Runnable examples live in [`examples/`](examples/) — one per module.

| Example | Run with | Covers |
|---------|----------|--------|
| `errors` | `cargo run --example errors` | Every `CliError` variant, `chain()`, and `render()` exit codes |
| `logging` | `cargo run --example logging -- -vv` | `logging::init` verbosity levels, stderr-only output |
| `output` | `cargo run --example output -- json` | `Renderable` trait, `make_table`, `DiagnosticsOutput`, pretty vs JSON |
| `prompts` | `cargo run --example prompts` | `input`, `password`, `confirm`, `select`, `multiselect`, cancellation |
| `ui` | `cargo run --example ui` | Alerts (`success`/`warning`/`info`/`error`), `Spinner`, intro/outro banners |
| `picker` | `cargo run --example picker --features picker` | Inline fuzzy picker with badges and footer hints |
| `tui` | `cargo run --example tui --features tui` | `FlexRenderable` component model + `top_bar`/`footer`/`prompt` widgets |
| `screen_canary` | `cargo run --example screen_canary --features tui` | `ScreenApp` on either surface; the probe used by `tools/altscreen-canary.sh` |
| `outline` | `cargo run --example outline --features tui` | `OutlineBuilder`/`OutlineState` fold-aware tree: keyed fold state, `fold_right`, `expand_all` |
| `text_area` | `cargo run --example text_area --features tui` | `TextArea` multi-line input: newline on `Enter`, wrapping, `desired_height`, line-scoped `Ctrl-u` |

## JSON errors

An app with a JSON mode opts in once, after parsing:

```rust
let cli: Cli = cli_kit::args::parse();     // clap errors honour --json too
cli_kit::output::set_format(opts.format);   // errors reaching main follow it
```

From then on, in JSON mode, an error that reaches `main`
(`flow::render_error` / `CliError::render`) is one document on **stdout**,
where the client is already reading, and nothing is printed on stderr:

```json
{"error": {"kind": "usage", "message": "…", "exit_code": 2, "details": {}}}
```

- `kind`: `usage` · `cancelled` · `auth` · `config` · `external` ·
  `internal` · `failed` (a flow already showed it: `Flow::fail`).
- `message`: the bare message, without the text prefix (`API error: `);
  for `internal`, the whole context chain.
- `exit_code`: the process exit code (2 usage, 130 cancelled, 1 otherwise).
- `details`: present only when the error carries data
  (`CliError::with_details`), e.g. an ambiguous lookup's candidates.

So in JSON mode stdout holds exactly one document whether the command
succeeded or failed, and the exit code says which. Without `set_format`,
errors stay styled text on stderr.

## TUI surfaces

`tui::screen::run` drives a `ScreenApp` on the surface it asks for:

| Surface | Where it draws | Use for |
|---------|----------------|---------|
| `Surface::AltScreen` (default) | Full window, alternate screen | Anything sensitive, anything long-lived. Content never enters scrollback and is discarded on exit; resizing is a plain repaint |
| `Surface::Inline { height }` | Rows below the shell prompt, primary screen | Short widgets that are part of the session and show nothing secret |

Inline output lands on the primary screen, so a terminal that reflows on resize
(kitty, VTE, alacritty) can push those rows into scrollback where the app can
neither find nor erase them — **never render secrets on the inline surface**. On
its first resize an inline app is moved to the alternate screen, because an
inline viewport cannot be repaired once the terminal has reflowed around it.

`tools/altscreen-canary.sh` is the regression test for all of this: it drives a
real kitty instance, resizes it, and asserts a canary string painted by the app
is gone from screen *and* scrollback afterwards.

## Design

- **Clean stdout** -- UI, prompts, and logging go to stderr so structured output stays pipeable
- **Dual rendering** -- Output types implement both pretty-printed and JSON formats via the `Renderable` trait
- **Prompt cancellation** -- Esc/Ctrl+C maps to `CliError::Cancelled` (exit code 130)
- **Terminal-aware** -- Footer hints collapse for narrow terminals; picker adapts to terminal height; synchronized rendering eliminates flicker
- **Explicit surfaces** -- A TUI app states whether it draws inline or on the alternate screen; the default is the one that cannot leak into scrollback

## Contributing

Contributions are welcome! Please open an issue or submit a pull request.

## License

[MIT](LICENSE)
