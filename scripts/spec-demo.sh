#!/usr/bin/env bash
# Render the spec demo, and verify it before publishing it.
#
# The GIF and tests/demo_verify.rs drive the same binary through the same
# beats. This runs the verification first and refuses to publish a GIF whose
# beats do not hold, because a demo that shows a defect is worse than no demo.
#
#   ./scripts/spec-demo.sh            verify, render, optimise, report
#   ./scripts/spec-demo.sh --verify   verification only, no render
#   KEEP=1 ./scripts/spec-demo.sh     leave the scratch home behind
set -euo pipefail

cd "$(dirname "$0")/.."
ROOT="$PWD"
SCRATCH="${TMPDIR:-/tmp}/essh-spec-demo.$$"
DEMO_HOME="$SCRATCH/home"
KEY="${SSH_KEY:-$HOME/.ssh/id_ed25519}"
OUT="docs/media/essh-spec-demo.gif"

# The fixture fleet from SPEC.md §2. Eleven hosts, and staging-web-01 is the
# one that does not answer: `scripts/fleet.sh` keeps that container stopped, so
# the offline row in the demo is drawn from a probe that actually failed.
HOSTS=(
  "web-01:127.0.0.11:prod:us-east:web"
  "web-02:127.0.0.12:prod:us-east:web"
  "web-03:127.0.0.13:prod:us-east:web"
  "db-01:127.0.0.14:prod:us-east:db"
  "db-02:127.0.0.15:prod:us-east:db"
  "cache-01:127.0.0.16:prod:us-west:cache"
  "edge-01:127.0.0.17:prod:us-east:edge"
  "build-01:127.0.0.18:ci:us-west:build"
  "monitor-01:127.0.0.19:prod:us-west:monitor"
  "staging-web-01:127.0.0.20:staging:us-east:web"
  "jump-01:127.0.0.21:prod:us-east:jump"
)
OFFLINE_HOST=staging-web-01

cleanup() { [ -n "${KEEP:-}" ] || rm -rf "$SCRATCH"; }
trap cleanup EXIT

need() { command -v "$1" >/dev/null || { echo "need $1" >&2; exit 1; }; }

# ── the fleet has to be the fixture, or nothing below means anything ──────
#
# Reachability is not enough. The staging that makes these hosts interesting
# lives in their running state, and a container restart keeps the filesystem
# and loses the processes — so the fleet comes back reachable and unstaged, and
# the verification below fails on a fact about the laptop rather than about
# essh. `fleet.sh check` reports that drift; `fleet.sh up` fixes it.
./scripts/fleet.sh check

# ── verify before rendering ───────────────────────────────────────────────
cargo build --release
echo "verifying the beats against docs/redesign/SPEC.md…"
cargo test --release --test demo_verify -- --nocapture
[ "${1:-}" = "--verify" ] && exit 0

# ── a throwaway home, so recording never touches a real ~/.essh ───────────
mkdir -p "$DEMO_HOME/.essh" "$DEMO_HOME/.ssh"
cp "$KEY" "$DEMO_HOME/.ssh/id_ed25519"
chmod 600 "$DEMO_HOME/.ssh/id_ed25519"
cp "$ROOT/target/release/essh" "$DEMO_HOME/essh"

cat > "$DEMO_HOME/.essh/config.toml" <<EOF
theme = "dark"
host_groups = []

[general]
default_user = "matt"
default_key = "$DEMO_HOME/.ssh/id_ed25519"
tofu_policy = "auto"
launcher = true
connect_timeout = 12

[session]
prefix_key = "ctrl-a"
max_concurrent = 9
EOF

for spec in "${HOSTS[@]}"; do
  IFS=: read -r name ip env region role <<<"$spec"
  cat >> "$DEMO_HOME/.essh/config.toml" <<EOF

[[hosts]]
name = "$name"
hostname = "$ip"
port = 2222
user = "matt"
tags = { env = "$env", region = "$region", role = "$role" }
EOF
done

# ── render ────────────────────────────────────────────────────────────────
need vhs; need ffmpeg
sed -e "s#@HOME@#$DEMO_HOME#g" -e "s#@OUT@#$SCRATCH/demo.gif#g" \
  docs/media/spec-demo.tape > "$SCRATCH/spec-demo.tape"
( cd "$ROOT" && vhs "$SCRATCH/spec-demo.tape" )

# ── optimise ──────────────────────────────────────────────────────────────
# gifsicle is not always present; ffmpeg's palettegen produces a smaller file
# with better colour on terminal captures anyway, because a TUI uses few
# colours and a generated palette spends all 256 on the ones in the frame.
if command -v gifsicle >/dev/null; then
  gifsicle -O3 --lossy=40 --colors 128 "$SCRATCH/demo.gif" -o "$OUT"
else
  ffmpeg -y -loglevel error -i "$SCRATCH/demo.gif" \
    -vf "split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4" \
    "$OUT"
fi

# A video alongside the GIF, at a quality target: without one ffmpeg turns a
# 2MB GIF into a 4MB MP4, which is a strange thing to offer as the smaller
# option. Distro builds of ffmpeg often ship without x264 (Fedora's
# `ffmpeg-free` does), so fall back to VP9 rather than fail at the last step.
SCALE="scale=trunc(iw/2)*2:trunc(ih/2)*2"
# Captured once rather than piped into `grep -q` per test: under `pipefail`,
# grep's early exit hands ffmpeg a SIGPIPE and the pipeline reports failure —
# so the encoder that *is* present reads as missing.
ENCODERS="$(ffmpeg -hide_banner -encoders 2>/dev/null || true)"
if grep -q " libx264" <<<"$ENCODERS"; then
  VIDEO=docs/media/essh-spec-demo.mp4
  ffmpeg -y -loglevel error -i "$SCRATCH/demo.gif" \
    -movflags faststart -pix_fmt yuv420p -c:v libx264 -preset slow -crf 28 \
    -vf "$SCALE" "$VIDEO"
elif grep -q " libvpx-vp9" <<<"$ENCODERS"; then
  VIDEO=docs/media/essh-spec-demo.webm
  ffmpeg -y -loglevel error -i "$SCRATCH/demo.gif" \
    -c:v libvpx-vp9 -pix_fmt yuv420p -b:v 0 -crf 36 -row-mt 1 \
    -vf "$SCALE" "$VIDEO"
else
  VIDEO=""
  echo "no h264 or vp9 encoder; skipping the video" >&2
fi

printf 'gif  %s  %ss\n' "$(du -h "$OUT" | cut -f1)" \
  "$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$OUT")"
[ -n "$VIDEO" ] && printf '%-4s %s\n' "${VIDEO##*.}" "$(du -h "$VIDEO" | cut -f1)"
