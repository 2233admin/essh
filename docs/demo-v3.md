# ESSH v0.4.0 recorded admin tour

The tour records the real release binary in a 192-column × 46-row terminal against
an isolated, loopback-only SSH server. It uses ephemeral keys and separate local
and remote tmux sockets. Existing ESSH sessions and SSH configuration are untouched.

[Watch the verified demo](https://github.com/matthart1983/essh/releases/download/v0.4.0/essh-v3-demo.mp4).

Connection and reconnect waits are shortened to 0.6 seconds. Command output and
deliberate dashboard/help reading pauses retain their timing. The original cast
retains the unedited timing; `speedups.json` documents each shortened interval.

## Record and render

Requirements: Python 3, tmux, OpenSSH client/server, Node.js, Chromium, and ffmpeg.
Build the release binary first, then run from the repository root:

```sh
cargo build --release
python3 scripts/record-tmux-demo.py
npm install --prefix /tmp/essh-demo-render @xterm/xterm playwright
NODE_PATH=/tmp/essh-demo-render/node_modules \
  CHROME_PATH=/path/to/chromium \
  node scripts/render-tmux-demo.cjs
```

`ESSH_BIN` can override the release binary. `ESSH_DEMO_OUT` overrides the default
`artifacts/demo` output directory for both scripts.

Outputs:

- `essh-v3-demo.mp4`: captioned video, rendered from the actual terminal recording.
- `replay.html`: standalone browser replay, with embedded terminal renderer and data.
- `essh-v3-demo.cast`: original timed terminal output (asciicast v2).
- `verification.json`: behavior assertions from the same recorded run.
- `chapters.json`, `edited-chapters.json`, `frame-*.png`, `rendered-frames.json`: chapter timings and replay evidence.

## Demonstrated features

- Filter Hosts and connect over real SSH.
- Storage: `df -hT` and `lsblk` inspect filesystems and block devices.
- Processes: `ps -eo pid,comm,%cpu,%mem --sort=-%cpu` finds busy processes.
- System activity: `vmstat 1 3` samples CPU, memory, swap and I/O.
- Network: `ss -lntu` and `ip -br address` inspect listeners and interfaces.
- F2: terminal → thin side strip → half dashboard → full dashboard → terminal.
- Live CPU, memory, disk, network and process telemetry; offline host browsing.
- Independent sessions, F3 picker, F5/F6 switching, and number selection.
- F4 Main menu with both remote sessions still running.
- Mouse click on Sessions and F1 navigation help.
- A deliberate SSH transport drop followed by `r` reconnect. A shell variable
  proves that the same remote shell survives the interruption.
- Ctrl+A, then `h`, directly opens the full dashboard.

The automated mouse action sends terminal mouse events at the Sessions control;
the caption identifies the action. This is a terminal replay, not a desktop camera
recording. Telemetry comes from the local demo host and varies between runs.

## v0.4.0 verification

The published video is 37.8 seconds, H.264, 1700 × 1200. All 25 behavior
checks and 14 rendered-output checks passed, along with full video decoding.
The half dashboard, admin command output and exported full-health frame were
visually reviewed. Initial metrics may show unavailable values while sampling
starts. The fixture leaves existing user sessions intact.
