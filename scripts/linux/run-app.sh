#!/usr/bin/env bash
# Runs the player headless inside the Linux image and photographs it — the
# evidence that the picture (libmpv's render API in a GL area) shows under the
# HTML interface, plays, pauses, seeks and closes. Started by in-container.sh
# (`scripts/linux/check.sh run-x11` / `run-wayland`), after the build.
#
#   x11      Xvfb with GLX (Mesa llvmpipe), no window manager, no compositor —
#            the harshest case for WebKit's transparency (docs/linux.md).
#            Driven with xdotool: hover, Space, L (+10 s), a click on the
#            seekbar, a hover over it (the thumbnail), Ctrl+Q.
#   wayland  A headless Weston with its GL renderer; screenshots only, since
#            nothing can inject input into a headless Weston.
#
# Output (screenshots, the app's stderr, mpv's log, frame differences) goes to
# /out/<mode>, which check.sh mounts from $FP_LINUX_OUT on the host.
#
# Knobs: FP_RUN_MEDIA (a file to play instead of the generated one),
# FP_RUN_SECONDS (how long to let it play before the first shot, default 8),
# FP_RUN_AO (an `ao=` for mpv.conf, e.g. `null` — the image has no sound server
# and mpv's `auto` already falls back to no audio, so this is for the day it
# does not), FP_RUN_HWDEC (an `hwdec=` for mpv.conf, e.g. `no`), FP_RUN_CONF (any
# further mpv.conf lines, `\n`-separated), FP_RUN_SIZE (the test video's size).
set -uo pipefail

mode="${1:-x11}"
out="/out/$mode"
mkdir -p "$out"
rm -f "$out"/*
app=/cache/target/debug/frameplayer
settle="${FP_RUN_SECONDS:-8}"

# A fresh profile per run: no remembered window size, position or settings.
export HOME=/tmp/fp-run-home
rm -rf "$HOME"
mkdir -p "$HOME/.config/app.frameplayer"
if [ -n "${FP_RUN_AO:-}" ]; then
  echo "ao=$FP_RUN_AO" >>"$HOME/.config/app.frameplayer/mpv.conf"
fi
if [ -n "${FP_RUN_HWDEC:-}" ]; then
  echo "hwdec=$FP_RUN_HWDEC" >>"$HOME/.config/app.frameplayer/mpv.conf"
fi
if [ -n "${FP_RUN_CONF:-}" ]; then
  printf '%b\n' "$FP_RUN_CONF" >>"$HOME/.config/app.frameplayer/mpv.conf"
fi

media="${FP_RUN_MEDIA:-}"
if [ -z "$media" ]; then
  # 60 s of moving picture with a running timecode (top-left of testsrc2) and
  # a tone, a keyframe every second so a seek lands where it is asked to.
  media=/tmp/fp-test.mp4
  ffmpeg -loglevel error -y -f lavfi -i testsrc2=size=${FP_RUN_SIZE:-1280x720}:rate=30 \
    -f lavfi -i sine=frequency=440:sample_rate=48000 -t 60 \
    -c:v libx264 -g 30 -pix_fmt yuv420p -c:a aac -shortest "$media"
fi

export LIBGL_ALWAYS_SOFTWARE=1
export NO_AT_BRIDGE=1
# The single-instance guard talks to D-Bus; a debug build may skip it.
export FP_NO_SINGLE_INSTANCE=1
export RUST_LOG="${RUST_LOG:-info}"

# Pixels that differ between two shots, inside the window's middle (the
# picture), ignoring the bars at the top and bottom.
differ() {
  local a="$1" b="$2" w h
  w=$(identify -format '%w' "$a"); h=$(identify -format '%h' "$a")
  compare -metric AE -fuzz 8% \
    <(convert "$a" -crop "$((w * 6 / 10))x$((h * 5 / 10))+$((w * 2 / 10))+$((h * 2 / 10))" png:-) \
    <(convert "$b" -crop "$((w * 6 / 10))x$((h * 5 / 10))+$((w * 2 / 10))+$((h * 2 / 10))" png:-) \
    null: 2>&1 || true
}

finish() {
  local status="$1"
  echo "exit=$status" >>"$out/summary.txt"
  find "$HOME" -name '*.log' -exec cp {} "$out/" \; 2>/dev/null
  echo "==> $out"
  cat "$out/summary.txt"
}

if [ "$mode" = x11 ]; then
  export DISPLAY=:99
  Xvfb :99 -screen 0 1600x1000x24 +extension GLX -nolisten tcp -noreset >"$out/xvfb.log" 2>&1 &
  xvfb=$!
  for _ in $(seq 1 50); do xdpyinfo >/dev/null 2>&1 && break; sleep 0.1; done
  export GDK_BACKEND=x11

  dbus-run-session -- "$app" "$media" >"$out/app.log" 2>&1 &
  runner=$!
  wid=""
  for _ in $(seq 1 100); do
    pid=$(pgrep -n -x frameplayer || true)
    [ -n "$pid" ] && wid=$(xdotool search --onlyvisible --pid "$pid" 2>/dev/null | head -1)
    [ -n "$wid" ] && break
    sleep 0.2
  done
  if [ -z "$wid" ]; then
    echo "no window appeared" >>"$out/summary.txt"
    kill "$runner" 2>/dev/null
    finish 1
    exit 1
  fi
  sleep "$settle"

  # Geometry is read before each step: "match video aspect ratio" reshapes
  # the window once the picture's size is known.
  geom() { eval "$(xdotool getwindowgeometry --shell "$wid")"; }
  shot() { geom; import -window root -crop "${WIDTH}x${HEIGHT}+${X}+${Y}" +repage "$out/$1.png"; }

  geom
  echo "window ${WIDTH}x${HEIGHT}+${X}+${Y}" >>"$out/summary.txt"
  # Keys go through XTEST to the focused window: there is no window manager
  # to give it focus, and GTK ignores the synthetic events `key --window`
  # would send instead.
  xdotool windowfocus --sync "$wid"
  shot 0-idle
  # (a) the controls come up under a moving pointer
  xdotool mousemove --window "$wid" $((WIDTH / 2)) $((HEIGHT / 2)); sleep 0.3
  xdotool mousemove --window "$wid" $((WIDTH / 2 + 20)) $((HEIGHT / 2 + 10)); sleep 0.6
  shot 1-playing-controls
  sleep 1.5
  shot 2-playing-later
  echo "playing, 1.5 s apart: $(differ "$out/1-playing-controls.png" "$out/2-playing-later.png") px differ" >>"$out/summary.txt"

  # (b) Space pauses: two shots a second apart are the same frame
  xdotool key space; sleep 1
  shot 3-paused
  sleep 1.5
  shot 4-paused-later
  echo "paused, 1.5 s apart: $(differ "$out/3-paused.png" "$out/4-paused-later.png") px differ" >>"$out/summary.txt"

  # (c) a seek while paused moves the picture: L (+10 s), then a click on the
  # seekbar near its start
  xdotool key l; sleep 1.5
  shot 5-after-L
  echo "paused, before/after L: $(differ "$out/4-paused-later.png" "$out/5-after-L.png") px differ" >>"$out/summary.txt"
  geom
  bar_y=$((HEIGHT - ${FP_RUN_BAR_FROM_BOTTOM:-71}))
  xdotool mousemove --window "$wid" $((WIDTH * 15 / 100)) "$bar_y"; sleep 0.4
  xdotool click 1; sleep 1.5
  shot 6-after-click
  echo "paused, before/after seekbar click: $(differ "$out/5-after-L.png" "$out/6-after-click.png") px differ" >>"$out/summary.txt"

  # (d) the hover thumbnail over the seekbar
  xdotool mousemove --window "$wid" $((WIDTH * 60 / 100)) "$bar_y"; sleep 0.3
  xdotool mousemove --window "$wid" $((WIDTH * 62 / 100)) "$bar_y"; sleep 2
  shot 7-seekbar-hover

  # play again, so the close happens mid-playback
  xdotool key space; sleep 1.5
  shot 8-resumed

  # (e) Ctrl+Q: the player's own quit, which destroys the window
  xdotool key ctrl+q
  status=124
  for _ in $(seq 1 30); do
    if ! kill -0 "$runner" 2>/dev/null; then wait "$runner"; status=$?; break; fi
    sleep 0.5
  done
  if [ "$status" = 124 ]; then
    echo "still running 15 s after Ctrl+Q; killing" >>"$out/summary.txt"
    kill "$runner" 2>/dev/null
  fi
  kill "$xvfb" 2>/dev/null
  finish "$status"
  exit 0
fi

if [ "$mode" = wayland ]; then
  export XDG_RUNTIME_DIR=/tmp/fp-xdg
  rm -rf "$XDG_RUNTIME_DIR"; mkdir -p "$XDG_RUNTIME_DIR"; chmod 700 "$XDG_RUNTIME_DIR"
  weston --backend=headless --renderer=gl --width=1600 --height=1000 --socket=wayland-9 \
    --idle-time=0 --debug --no-config >"$out/weston.log" 2>&1 &
  weston=$!
  for _ in $(seq 1 50); do [ -S "$XDG_RUNTIME_DIR/wayland-9" ] && break; sleep 0.1; done
  export WAYLAND_DISPLAY=wayland-9 GDK_BACKEND=wayland
  unset DISPLAY

  cd "$out" || exit 1
  dbus-run-session -- "$app" "$media" >"$out/app.log" 2>&1 &
  runner=$!
  sleep "$settle"
  weston-screenshooter >>"$out/weston.log" 2>&1; sleep 1.5
  weston-screenshooter >>"$out/weston.log" 2>&1; sleep 0.5
  i=1
  for f in "$out"/wayland-screenshot-*.png; do
    [ -f "$f" ] && mv "$f" "$out/$i-shot.png" && i=$((i + 1))
  done
  if [ -f "$out/1-shot.png" ] && [ -f "$out/2-shot.png" ]; then
    echo "playing, 1.5 s apart: $(differ "$out/1-shot.png" "$out/2-shot.png") px differ" >>"$out/summary.txt"
  fi
  # No input to send a quit with: SIGTERM, and the exit code says only that
  # the process was alive to receive it.
  kill "$(pgrep -n -x frameplayer)" 2>/dev/null
  status=124
  for _ in $(seq 1 20); do
    if ! kill -0 "$runner" 2>/dev/null; then wait "$runner"; status=$?; break; fi
    sleep 0.5
  done
  kill "$weston" 2>/dev/null
  finish "$status"
  exit 0
fi

echo "unknown mode: $mode (x11 | wayland)" >&2
exit 2
