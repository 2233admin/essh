# essh v3 — tmux workspace

The standalone [design reference](index.html) has four working view buttons and
no JavaScript or font downloads. Its sample metrics are illustrative. The app
uses Rust/ratatui on a terminal character grid.

## Architecture

The local `tmux -L essh-v3` server owns the global host list, session windows,
status bar, help and switcher popups. Window 0 is hosts. Each connection gets a
stable UUID, a local window, and its own **remote tmux session** named
`essh-<uuid>` in the remote `tmux -L essh` server. Each remote session starts with
one shell window. This replaces the original proposal's shared remote `essh`
session: independent remote sessions prevent two clients on one host from
changing each other's selected window.

OpenSSH carries the terminal directly; remote tmux preserves the shell and its
scrollback across SSH drops and local restarts. The app does not emulate a
terminal or translate remote shell output. Local and remote tmux servers use
separate sockets and do not read the user's tmux configuration. OpenSSH still
uses the user's SSH configuration, authentication and known-hosts policy.
Native essh authentication/audit/security settings belong to `essh legacy`;
configure the v3 transport through OpenSSH.

Saved IDs and host connection settings live in `~/.essh/tmux-v3/`. Closing the
local client leaves both layers running. If the local server is lost, launch
restores saved entries without connecting; Enter reconnects the selected entry.
A reconnect creates a replacement shell if the remote session has ended, and
says so in the reconnect hint. IDs identify independent sessions on the same
host. A saved record never proves that remote processes remain alive.

Requirements: local tmux 3.3+, OpenSSH, and tmux on each destination. Linux `/proc`
and `ss` provide optional metrics; missing metrics do not prevent shell use.
The new default is `essh`; `essh legacy` opens the previous native interface.
Existing explicit CLI commands remain available during migration.

## Navigation

| Key | Action |
| --- | --- |
| F1 | Contextual help; F1 or Esc returns |
| F2 | Full health dashboard; F2 or Esc returns to the terminal |
| F3 | Session switcher |
| F4 | Main menu (Hosts), keeping remote sessions running |
| F5 / F6 | Previous / next SSH session, wrapping and skipping Hosts |

The two-row tmux bar has session tabs and clickable navigation controls. Views
inside popups repeat those controls. Buttons shorten in small terminals.
Mouse support includes row selection, double-click activation, wheel scrolling,
pane focus and divider resizing. Keyboard navigation works without a mouse.

Hosts: arrows/jk select, Enter connects or reattaches, a adds, e edits, / filters
names/tags, q detaches and keeps jobs running. Tab focuses navigation controls;
Enter activates one. On the main menu, Esc returns to the remembered session.

Sessions: arrows/jk select, Enter switches, 1–9 jumps, / filters, n creates a new
session, x explicitly confirms remote termination. More than nine sessions are
available by scrolling. Session numbers are saved alongside UUIDs across restart.
A disconnected entry is retained as unverified; failed termination retains it.

Help: scroll with arrows/wheel; F1 or Esc returns. It documents mouse actions,
health, session lifetime, remote splits, copy behavior, and compatibility keys.

F1–F6 are reserved by essh. To send a reserved key to the remote program, press
Ctrl+A, release, then v, then the desired key. Ctrl+A twice sends literal Ctrl+A.
The remote tmux prefix remains Ctrl+B. Esc and Ctrl+C in the terminal continue
to go to the remote program. Existing Ctrl+A session/help/menu bindings remain.
Ctrl+A then lowercase m toggles the optional side strip.

## Full health dashboard

F2 opens a full-size local popup, leaving the remote terminal running. Its
header identifies the host, session, connection state and sample age. F5/F6 and
session-tab clicks change the host without mixing the previous host's readings.
Without a session, Health directs the user to connect from the main menu.

Panels:

- CPU total/per-core usage, history, and 1/5/15-minute load averages.
- Memory total/used/available, swap and recent usage history.
- Network RX/TX rates and history, with interface selection (i).
- Storage capacity by filesystem, plus physical-device I/O rates when available.
- Processes: PID, command name, CPU and memory; c/m switches the sort order.
- TCP peers and interval RX/TX rates.

Tab/Shift+Tab focuses a panel. Arrows or the mouse wheel scroll it. At 120+
columns and sufficient height the six panels form a grid. Smaller terminals use
selectable panels, including at 60×20. F2/Esc returns to the shell; b switches to
the optional 38-column side strip. The strip still requires a 100-column window.

Process CPU is the lifetime average reported by ps, not instantaneous CPU.
Only command names are collected, not command arguments. Network rates use
counter deltas; peer counters exclude short-lived sockets between samples and
need not equal interface totals. Physical I/O excludes virtual/aggregate devices
to avoid counting the same transfer twice. Units are bytes/s or explicitly GiB.

## Collection and availability

Direct hosts use TCP probes on their configured port, with bounded concurrency
and a five-second interval after each sweep. Proxied hosts do not get misleading
direct-connect failure verdicts. Latency includes DNS and TCP connection setup;
it is not remote command runtime. Missing/proxied latency is unavailable.

Health uses the existing OpenSSH control socket, with no fallback authentication
connection. A per-session worker is shared by dashboard clients. Each view renews
an expiring lease; closing or crashing the last view stops the worker. The normal
sampler pauses while Health is collecting, avoiding duplicate detailed probes.
Newly built samplers resume light status/strip sampling after Health closes.
Existing SSH wrappers from an older binary retain their old light sampler until
their next reconnect; refreshing navigation does not interrupt those sessions.

CPU/memory/network counters refresh about once a second. Processes/filesystems
refresh roughly every three seconds. The status-only sampler runs roughly every
five seconds. Linux /proc and ss supply optional metrics; unsupported metrics
remain unavailable independently. A disconnected dashboard retains the previous
sample with its age and connection state; data becomes stale after twelve seconds.
No unavailable reading is represented as a measured zero.

The status bar shows the current connection's rates and latency, plus the clock.
Hosts has no current remote connection, so it shows the clock only. The navigation
row remains visible and clickable. Long session lists overflow into the switcher.

## Updating a running workspace

After building, `essh tmux refresh` reapplies bindings, saves the current session
numbers, and restarts only the main-menu UI. SSH windows and remote jobs remain
running. The dashboard worker uses the new binary immediately.

## Validation

Run `cargo fmt --check`, `cargo test`, and
`cargo clippy --all-targets --all-features -- -D warnings`.

After `cargo build`, run `python3 scripts/test-tmux-v3.py`. The fixture uses an
isolated loopback sshd, ephemeral keys and separate tmux servers. It exercises
F1–F6, full health data, mouse controls, side-strip compatibility, reconnect,
resizing, saved IDs/order, explicit termination and failed-kill retention.
Unit rendering checks cover compact and wide panel layouts and navigation hit
regions. The standalone HTML is a design reference containing sample data.
