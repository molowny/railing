# railing

Building blocks for Rust command-line apps: step-by-step output, interactive
prompts, tables, JSON output, structured errors, logging.

## Install

```toml
[dependencies]
railing = { version = "0.5", features = ["flow"] }
```

## Features

The core needs no features: terminal handling, raw mode, key input, styling.
Everything else is opt-in.

| Feature   | Adds                                             |
|-----------|--------------------------------------------------|
| `flow`    | The output flow, prompts, `CliError`             |
| `fuzzy`   | Fuzzy matching in pickers and table filters      |
| `table`   | Tables (`Grid`)                                  |
| `output`  | JSON output and JSON errors                      |
| `args`    | clap parsing and shell completion                |
| `logging` | tracing subscriber setup                         |
| `paths`   | XDG paths and config file lookup                 |
| `full`    | All of the above                                 |

Interactive terminal features are Unix-only. Elsewhere, output falls back to
plain text.

## Examples

```sh
cargo run --features full --example prompts
```

See [`examples/`](examples/) for the rest.

## License

[MIT](LICENSE)
