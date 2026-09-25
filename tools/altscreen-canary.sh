#!/usr/bin/env bash
#
# Regression test for the TUI surface contract. Terminal-behavior bugs (reflow,
# scrollback, alternate screen) cannot be caught by `cargo test`, so this drives
# a real kitty instance over its remote-control socket.
#
#   AltScreen: after resizing and quitting, the canary string must appear
#              NOWHERE in the terminal buffer (screen + scrollback), and the
#              shell output printed before the app must still be there.
#   Inline:    the widget renders inline, and the first resize moves it to the
#              alternate screen (exactly one frame on screen from then on).
#
# Usage: cli-kit/tools/altscreen-canary.sh [altscreen|inline|both]
# Requires: kitty (any window; a separate minimized instance is used).

set -u
MODE="${1:-both}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
CANARY="CANARY-SECRET-7Q4Z"
FAILED=0

command -v kitty >/dev/null || { echo "SKIP: kitty not installed"; exit 0; }
cargo build -q -p cli-kit --features tui --example screen_canary || exit 1
BIN="$ROOT/target/debug/examples/screen_canary"

run_case() {
  local mode="$1"
  local sock="unix:/tmp/kitty-canary-$$-$mode"
  echo "== surface: $mode =="

  kitty -o allow_remote_control=yes --listen-on "$sock" \
        -o initial_window_width=140c -o initial_window_height=40c \
        -o enabled_layouts=splits -o scrollback_lines=4000 \
        --start-as=minimized --title "canary-$mode" \
        bash -c "for i in \$(seq 1 40); do echo \"shell output line \$i — long enough that it rewraps when the window gets narrower\"; done; '$BIN' $mode; echo EXITED; sleep 60" \
        >/dev/null 2>&1 &
  local kpid=$!
  for _ in $(seq 40); do kitty @ --to "$sock" ls >/dev/null 2>&1 && break; sleep 0.25; done
  local id
  id=$(kitty @ --to "$sock" ls 2>/dev/null |
       python3 -c 'import json,sys; print(json.load(sys.stdin)[0]["tabs"][0]["windows"][0]["id"])') || return 1
  # A second split gives kitty something to resize the probe against, without
  # involving the window manager.
  kitty @ --to "$sock" launch --location=vsplit -- sleep 300 >/dev/null 2>&1
  sleep 1.5

  local frames_ok=1
  for inc in -8 -8 -8 10 10; do
    kitty @ --to "$sock" resize-window --match "id:$id" --axis horizontal --increment $inc >/dev/null 2>&1
    sleep 0.7
    local n
    n=$(kitty @ --to "$sock" get-text --match "id:$id" --extent screen | grep -c '^TOP ')
    [ "$n" = "1" ] || { echo "  FAIL: $n frames on screen after resize $inc (want 1)"; frames_ok=0; }
  done
  [ "$frames_ok" = "1" ] && echo "  ok: one frame on screen after every resize"

  kitty @ --to "$sock" send-text --match "id:$id" q >/dev/null 2>&1
  sleep 1.2
  local left shell_lines
  left=$(kitty @ --to "$sock" get-text --match "id:$id" --extent all | grep -c "$CANARY")
  shell_lines=$(kitty @ --to "$sock" get-text --match "id:$id" --extent all | grep -c 'shell output line')

  if [ "$mode" = "altscreen" ]; then
    if [ "$left" = "0" ]; then
      echo "  ok: canary gone from screen and scrollback"
    else
      echo "  FAIL: canary still present $left time(s) after quit"; FAILED=1
    fi
    if [ "$shell_lines" -ge 20 ]; then
      echo "  ok: shell output preserved ($shell_lines lines)"
    else
      echo "  FAIL: shell output lost (only $shell_lines lines left)"; FAILED=1
    fi
  else
    # Inline deliberately leaves its last frame behind — that is why it must
    # never be used for secrets. Only the frame count above is asserted.
    echo "  note: inline leaves $left canary line(s) behind, as documented"
  fi
  [ "$frames_ok" = "1" ] || FAILED=1

  kitty @ --to "$sock" close-os-window >/dev/null 2>&1
  kill $kpid 2>/dev/null
  wait $kpid 2>/dev/null
}

case "$MODE" in
  altscreen|inline) run_case "$MODE" ;;
  both) run_case altscreen; run_case inline ;;
  *) echo "usage: $0 [altscreen|inline|both]"; exit 2 ;;
esac

[ "$FAILED" = "0" ] && echo "PASS" || echo "FAIL"
exit $FAILED
