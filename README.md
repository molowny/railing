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
| `error` | Categorized `CliError` enum (Usage, Cancelled, Auth, Config, External, Internal) with styled rendering and proper exit codes |
| `logging` | Tracing subscriber bootstrap with `-v`/`-vv` verbosity levels |
| `output` | `Renderable` trait for pretty/JSON dual rendering, table helpers, `GlobalOpts` |
| `prompts` | Text input, password, and confirmation prompts with automatic cancellation handling |
| `ui` | Colored alerts, spinners, intro/outro banners |

### Optional

| Module | Feature flag | Description |
|--------|-------------|-------------|
| `tui` | `tui` | Ratatui-based component model with flex layout, paint utilities, theme constants, and widgets |
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

## Design

- **Clean stdout** -- UI, prompts, and logging go to stderr so structured output stays pipeable
- **Dual rendering** -- Output types implement both pretty-printed and JSON formats via the `Renderable` trait
- **Prompt cancellation** -- Esc/Ctrl+C maps to `CliError::Cancelled` (exit code 130)
- **Terminal-aware** -- Footer hints collapse for narrow terminals; picker adapts to terminal height; synchronized rendering eliminates flicker

## Contributing

Contributions are welcome! Please open an issue or submit a pull request.

## License

[MIT](LICENSE)
