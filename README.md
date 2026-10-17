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
cli-kit = { version = "0.1", features = ["tui"] }
```

## Modules

### Core

| Module | Description |
|--------|-------------|
| `flow` | One command as one clack prompt group (`┌ … ◇ … └ ✔`): `Flow` (live or plain), `Grid` tables boxed inside steps (`table_box` for an app with its own gutter), checks, error close via `render_error`; `browse_table` opens a long list as an interactive grid inline in the flow under the app's `Browse` policy (`docs/cli.md` §8.1.1) |
| `completion` | Static shell completion scripts from the app's clap command tree |
| `args` | `args::parse::<Cli>()`: clap parsing whose errors follow the JSON error contract when the arguments ask for JSON |
| `error` | Categorized `CliError` enum (Usage, Cancelled, Auth, Config, External, Internal) with styled rendering, proper exit codes, and a JSON form (`kind`, `to_json`, `with_details`) |
| `logging` | Tracing subscriber bootstrap with `-v`/`-vv` verbosity levels |
| `output` | `Renderable` trait for pretty/JSON dual rendering, table helpers, `GlobalOpts`, `set_format` (opt into JSON errors) |
| `prompts` | Text input, password, confirmation, select, and the filtered picker (`pick`), with automatic cancellation handling |
| `fuzzy` | `Fuzzy`: the `nucleo-matcher` scorer shared by the picker and the TUIs |
| `ui` | Colored alerts, spinners, intro/outro banners |
| `glyph` | The flow's marks (`◆ ◇ ▲ ■`, `│`, `└`) and the muted tone as plain strings, for an app drawing the flow's look on a terminal of its own (slim core) |
| `spin` | `Spin`: the pulsing shape and the glow as strings at a steady `PACE` — what `ui::Spinner` and `Wait` animate, and what an app on its own terminal animates itself (slim core) |

### Optional

| Module | Feature flag | Description |
|--------|-------------|-------------|
| `tui` | `tui` | Ratatui-based component model with flex layout, paint utilities, theme constants, widgets, and the [`screen`](#tui-surfaces) app runner |

## Feature flags

| Flag | Enables | Extra dependencies |
|------|---------|-------------------|
| `full` (default) | Everything but the slim core: flows, prompts, output, errors, logging, completion | `cliclack`, `clap`, `comfy-table`, `tracing`, `serde`, … |
| `tui` | TUI components and layout system | `ratatui`, `crossterm`, `unicode-width` |

Without `full` (`default-features = false`) cli-kit is only its slim core,
`glyph` and `spin`, over `console`: the flow's look as strings, for a process
that draws on a terminal it did not inherit and must keep its dependency tree
small — `sealward-askpass` draws the unlock prompt as a step of the client's
flow this way. Colour follows `console`'s stderr flag, as everywhere in
cli-kit; such an app sets it for the terminal it draws on.

## Examples

Runnable examples live in [`examples/`](examples/) — one per module.

| Example | Run with | Covers |
|---------|----------|--------|
| `errors` | `cargo run --example errors` | Every `CliError` variant, `chain()`, and `render()` exit codes |
| `logging` | `cargo run --example logging -- -vv` | `logging::init` verbosity levels, stderr-only output |
| `output` | `cargo run --example output -- json` | `Renderable` trait, `make_table`, `DiagnosticsOutput`, pretty vs JSON |
| `prompts` | `cargo run --example prompts` | `input`, `password`, `confirm`, `select`, `multiselect`, cancellation |
| `ui` | `cargo run --example ui` | Gutter log lines (`ui::log`), `Spinner`, intro/outro banners |
| `boxes` | `cargo run --example boxes` | Every box a flow draws, in one run: `flow.table`, `Grid::kv`, `text_box`, `flow.note`, `flow.checks`, then the frozen `browse_table` (`-- no-grid` skips it) |
| `spinner` | `cargo run --example spinner` | `Flow::task` / `ui::Spinner` in every `SpinnerStyle` (star, diamond, square, circle) one after another: the pulsing shape and the glow, a task that updates and ends as `◇`, then one cleared (`-- slow` to watch; `-- square` for one style) |
| `select` | `cargo run --example select` | `prompts::select` over four rows; `-- pick` shows `prompts::pick` over the same rows |
| `multiselect` | `cargo run --example multiselect` | `prompts::multiselect`, all selected to start |
| `pick` | `cargo run --example pick` | `prompts::pick` over 60 rows: filtering, scrolling, the count and filter in the box title |
| `tui` | `cargo run --example tui --features tui` | `FlexRenderable` component model + `top_bar`/`footer`/`prompt` widgets |
| `screen_canary` | `cargo run --example screen_canary --features tui` | `ScreenApp` on either surface; the probe used by `tools/altscreen-canary.sh` |
| `outline` | `cargo run --example outline --features tui` | `OutlineBuilder`/`OutlineState` fold-aware tree: keyed fold state, `fold_right`, `expand_all` |
| `data_grid` | `cargo run --example data_grid` | `Flow::browse_table` over 200 rows, inline: scroll, filter, sort, frozen on `q`; `-- 5` fits, `-- 200 never` is `--no-pager` |
| `text_area` | `cargo run --example text_area --features tui` | `TextArea` multi-line input: newline on `Enter`, wrapping, `desired_height`, line-scoped `Ctrl-u` |

## Asking for a left-out argument

A command that needs something it can list (an item, a run, a profile) asks
for it when it is left out — but only where asking cannot hang a script or
end up in captured output:

```rust
let id = match id {
    Some(id) => id,
    None if flow.can_ask() => prompts::pick("Run", &rows)?, // rows: (value, label, hint)
    None => return Err(CliError::needs("app status", "run", "`app ls` lists them")),
};
```

- `Flow::can_ask()` — the frame is live (the app's own policy) and stdin is a
  terminal.
- `prompts::pick` — a filtered select over owned rows. It shows `Rows::Auto` rows at a
  time — half the terminal, between 5 and 25; `Picker::new(prompt).rows(Rows::Fixed(n))`
  sets another number for one call. It draws in the browsed grid's box: the filter is a chip
  on the title, the count is at its right (`1–12 of 60`, `6 of 60 match · 1–6`), every row
  shows its hint, and the controls are their own `◇` line. Typing filters the labels with
  `Fuzzy` — a space separates words; matches rank best first. Esc clears what was typed,
  then cancels. Once chosen, the box collapses to the prompt and its answer.

```
◆  Item ▸ /home 1  ───────── 6 of 60 match · 1–6 ─╮
│                                               │
│ › Homelab/service-10  ssh key                 │
│   Homelab/service-13  note                    │
│   Homelab/service-16  login                   │
│   …                                           │
├───────────────────────────────────────────────╯
│
◇  ↑↓ move · enter pick · esc clear
```
- `CliError::needs(cmd, noun, how)` — the usage error (exit 2) off a terminal:
  `` `app status` needs a run — `app ls` lists them ``.

Pickers choose; they never skip a confirmation.

Ctrl-C at any prompt is a cancel, not a kill: the prompt closes with
`Operation cancelled.`, the cursor comes back, and the call returns
`CliError::Cancelled` (exit 130). SIGINT is ignored only while a prompt reads
keys; the app's own handler, if any, is put back after.

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

## Shell completions

`ksef`, `hackbox`, `plmt`, and `sealward` all provide:

```sh
ksef completions bash
```

Replace `ksef` with any of those apps. Supported shells are `bash`, `zsh`,
`fish`, `powershell`, and `elvish`. The command writes only a shell script to
stdout, including when JSON or quiet flags are set, and needs no configuration,
credentials, or running services. These are static completions for the clap
command tree (commands, flags, and declared choices), not live application data.
Wright is not included yet.

### Shell setup

For Bash, add this to `~/.bashrc` (or run it in the current shell):

```bash
for app in ksef hackbox plmt sealward; do
    if command -v "$app" >/dev/null 2>&1; then
        source <("$app" completions bash)
    fi
done
```

For Zsh, generate completion files:

```zsh
mkdir -p ~/.zsh/completions
for app in ksef hackbox plmt sealward; do
    if command -v "$app" >/dev/null 2>&1; then
        "$app" completions zsh > ~/.zsh/completions/_"$app"
    fi
done
```

Add `fpath=(~/.zsh/completions $fpath)` to `~/.zshrc` before your existing
`compinit` call. If you do not already initialize completion, follow it with
`autoload -Uz compinit; compinit`.

For Fish, run this in Fish:

```fish
mkdir -p ~/.config/fish/completions
for app in ksef hackbox plmt sealward
    if command -q $app
        $app completions fish > ~/.config/fish/completions/$app.fish
    end
end
```

For PowerShell, add `ksef completions powershell | Out-String | Invoke-Expression`
to your `$PROFILE`, repeating for each app. For Elvish, add
`eval (ksef completions elvish | slurp)` to your rc file for each app.

Regenerate saved scripts after upgrading an app, then start a new shell.
The Bash and PowerShell/Elvish startup snippets regenerate on each shell launch.

### App integration

Use `cli_kit::completion::Shell` as a clap `#[arg(value_enum)]` argument on a
`Completions { shell }` subcommand. Immediately after parsing, before logging
or configuration loading, handle it with:

```rust,ignore
if let Commands::Completions { shell } = &cli.command {
    return cli_kit::completion::print::<Cli>(*shell);
}
```

The helper derives the executable name and command structure from
`Cli::command()` and reports stdout write failures as `CliError`.
