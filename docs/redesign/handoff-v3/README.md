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

## Screens and keys

Only hosts and terminal are primary screens. Switcher/help are popups. There is
no dashboard, broadcast, palette, file browser, forwarding screen or fleet view.

Hosts: `↑↓`/`jk` move, Enter connects or selects an existing session, `/` filters
host names and tags, `a` adds a host, `e` edits a host, `?` opens help, `q` detaches.
Host forms save to essh configuration; editing an SSH alias creates an essh
connection override. SSH config entries themselves are not rewritten.

Local prefix is **Ctrl+A**:

| Key after prefix | Action |
| --- | --- |
| 1–9 | Select local session number |
| s | Switcher; arrows/Enter, 1–9, n new, x kill, Esc close |
| n | Host picker for an independent new session |
| m | Toggle metrics pane in the current window |
| d | Host list |
| q | Detach the local client |
| x | Confirm and terminate the selected remote session |
| ? | Help |
| Ctrl+A | Send literal Ctrl+A |

The remote prefix remains **Ctrl+B**. These local bindings replace corresponding
tmux defaults; other tmux keys remain available. `x` requires typing `yes` because
it terminates remote programs. Failed termination retains the local entry; it
never reports that an unreachable session was killed.

## Reachability and metrics

Direct hosts get TCP probes on their configured port, every five seconds after
the preceding sweep completes, with at most eight concurrent probes. The last
ten successful measurements feed the latency graph. Proxied hosts show
`via proxy · unknown` until their SSH transport is established; direct TCP
failure is not evidence of proxy failure. DNS resolution can delay a probe
worker but cannot block keyboard input.

- Green: successful direct probe or current SSH connection.
- Red: saved connection whose current reachability is unverified/failed.
- Dim open dot: no saved session and no successful probe.
- Reconnect text: **remote state unknown**, never “tmux still running”.

The status bar shows the active connection's network rates and direct-host TCP
connect latency (which includes DNS resolution), plus the clock. Hosts shows only
the clock; it has no current remote host. Missing or stale values are `—` or
“metrics unavailable”, never fabricated zeroes. Successful metrics are considered
stale after twelve seconds.

Metrics reuse each interactive OpenSSH ControlMaster connection. A private socket
directory has owner/mode validation. The sampler checks that the master exists
and disables fallback connections. It samples network counters every five seconds;
when that window's strip is visible to an attached client it also samples CPU,
memory, root-filesystem usage and TCP peers about once per second. Sampling and
subprocess timeouts are bounded; the sampler stops when the SSH child exits.

Network totals aggregate non-loopback interfaces. Peer rates use `ss -tinH`
`bytes_sent`/`bytes_received` counter deltas keyed by endpoint pair, then aggregate
by peer. Queue sizes are not rates. A new socket or counter reset needs another
sample. Short-lived sockets between samples are not captured; missing counters
show an unavailable/empty state. Peer totals need not equal interface totals.

The strip is an explicit 38-column **local pane belonging to its connection
window**, off by default. Switching windows reveals that window's own strip state.
It does not magically migrate between windows. Hidden windows use light probes.

## Character-grid layout

Palette: terminal `#080b0d`, status `#0d1316`, selected row `#10171b`, track
`#14191d`, square border `#1c2429`, primary `#cbd5dd`, secondary `#8b9aa6`,
dim `#5b6b7a`, cyan `#4fd1c5`, green `#35d07f`, red `#ef5f52`.
No gradients, rounded borders or font-size dependencies. Host tables use a cyan
selection gutter; RX is green, TX cyan, utilization bars solid green. tmux owns
the one-row status bar; pixel borders/shadows and shell prompt colors in HTML
are illustrative, not control over a remote shell's theme.

At 105+ available columns hosts includes USER@ADDRESS; below that it hides that
column. Below 65 columns it also hides LAST and shortens HOST. Rows scroll with
the selection; long values clip inside their columns. The strip opens only at
100+ columns and shows a compact resize hint if narrowed below 26. Popups use
90% of the terminal width so they fit smaller terminals. Sessions beyond 9 are
reachable through the scrolling switcher. Help wraps to the terminal width.

## Validation

Unit tests cover counter deltas/resets and peer rates. Run `cargo test`,
`cargo fmt --check`, and `cargo clippy --all-targets --all-features -- -D warnings`.

After `cargo build`, run `python3 scripts/test-tmux-v3.py`. It starts an isolated
loopback sshd with ephemeral keys and separate local/remote tmux servers, then
checks navigation, metrics, two independent sessions on one host, forced SSH
disconnection, persistent shell state on reconnect, narrow layout, quit, saved
ID restoration and confirmed remote termination. It requires local sshd and a
login account that permits public-key authentication. It cleans up its fixture
servers and never changes the user's SSH configuration.

Sources: [tmux manual](https://man.openbsd.org/tmux),
[OpenSSH manual](https://man.openbsd.org/ssh),
[ss manual](https://man7.org/linux/man-pages/man8/ss.8.html).
