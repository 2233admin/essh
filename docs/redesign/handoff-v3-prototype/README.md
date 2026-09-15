# Handoff: essh v3 — interactive prototype

## Overview

essh v3 is a thin layer over **tmux** for people who work across several SSH hosts: a host list, a session
switcher, and a skinned status bar with an optional **netwatch**-style side strip. Nothing else.

This package contains the **working prototype** — a keyboard-driven, stateful model of the whole app — plus the
static design reference and the full visual spec. Build from the prototype's behavior and the spec's tokens.

## Files

- `ESSH v3 Prototype.dc.html` — **the behavioral reference.** Open in a browser (keep `support.js` beside it),
  click the terminal, use the keys. Every interaction listed below works; state persists across reloads.
- `ESSH v3 tmux.dc.html` — static design reference (the four states at rest, with note cards).
- `DESIGN.md` — the visual spec: screens, tokens, TUI/tmux translation table, removals list. Read it alongside
  this file; this file does not repeat it.
- `support.js` — runtime for the two HTML files. Not part of the product.

Both `.dc.html` files are **design references created in HTML**, not production code. Recreate in the target
environment (Rust/ratatui for the list and popups, `tmux.conf` for the status line, a metrics helper for the
strip). Do not copy the HTML or its JavaScript into the app.

## Fidelity

**High-fidelity** for palette, semantics, column order, copy and behavior. Pixel geometry does not transfer —
`ch` widths are cell counts; express size with bold/dim/color. See `DESIGN.md` → "Translating to a TUI / tmux".

## Prototype scope — what is real and what is faked

Real (implement as shown):
- all navigation, key handling, popups, forms, confirmation, filter, quit/relaunch flow
- session lifecycle: create-or-reattach, switch, kill (with confirm), reconnect after drop
- status bar composition and the rule that it never changes shape between screens
- strip layout and update cadence (1 Hz)

Faked (replace with real sources):
- the shell: a tiny command table (`uptime uname hostname whoami pwd ls df free top clear exit`) — the real
  thing is the remote tmux pane
- metrics: deterministic sine waves seeded by host name — the real thing reads `/proc/net/dev`, `/proc/stat`,
  `free`, `df`, `ss -tn` over the existing connection
- reachability: a static state per host, `r` flips edge-01 to reachable — the real thing pings port 22 every 5s
- persistence: browser `localStorage` — the real thing is tmux's own server state plus `~/.essh/hosts.toml`

## Screens and states

Four screens/overlays. The status bar is present on all but the quit screen.

### Host list (launch screen)

- Rows sorted as configured (no auto-sort). Selected row: bg `#10171b`, cyan `▌` marker, white bold name.
- Click selects, double-click connects (pointer environments only).
- **Empty filter result** shows `no hosts match "<text>"` in `#3f4b52` inside the table.
- Footer hints are static; the right-hand explainer reads
  `TMUX = windows open on that host · sessions keep running after quit`.

### Session

- Left: the tmux pane. Scrollback is bottom-anchored (newest line at the bottom, prompt last). Keep the last
  40 lines in the prototype; the real pane keeps whatever tmux keeps.
- Prompt: `matt@host` green · `:` `#3f4b52` · `~` cyan · `$` `#3f4b52` · typed text `#cbd5dd` · cyan block cursor
  blinking at 1 Hz (step, not fade).
- **Unreachable host:** the prompt is replaced by one red line
  `── connection to <host> lost · tmux session alive · r reconnect ──`. Typing is ignored; `r` reconnects and
  the prompt returns with scrollback intact.
- Right (optional): the netwatch strip, toggled with `^A m`, which **cycles three modes**:
  1. **off** — pane takes the full width (`1fr`)
  2. **strip** — 300px / 38 cols (`1fr 300px`): host + eth0, RX/TX with 27-glyph histories, cpu/mem/disk bars,
     rtt sparkline, PEER RX TX table, footer `^A m half · ^A h full page`
  3. **half** — 50/50 (`1fr 1fr`): the current host's health two-up beside the terminal — header (dot, host,
     user@addr, `up N days`), RX/TX with 26-glyph histories at 18px, then cpu bar + 24-glyph history + mem +
     load on the left and disk per mount + rtt on the right, then PEER table and COMMAND CPU RSS top-4.
     Footer `^A m hide · ^A h full page`. Unreachable host: one dim line `<state> · last reading, not live`.
  The strip/half always shows the **host of the active session**. Mode persists across session switches.

### Health (`h` in the list · `^A h` in a session) — one host, full page

The side strip expanded to the whole frame, for **a single host**: the one selected in the list, or the one
you're in when opened from a session. Never a fleet table.

- Header: reachability dot · host bold cyan · `user@addr` dim · `│` · `reachable`/`unreachable`/`offline` ·
  `N tmux` · right hint `←→ other host`.
- Row 1 (two columns): `RX eth0` / `TX eth0` with rate on the right and a 44-glyph braille history at 22px
  (green / cyan).
- Row 2 (three columns, `#14191d` rule above and below): **cpu** bar + 24-glyph cpu history (0–100 fixed
  ceiling) + `load` three values + `up` N days · **mem** bar + `N GiB of 62 GiB · 21 GiB cache`, `swap` bar,
  `rtt` 16-glyph sparkline + ms · **disk** one flat bar per mount (`/`, `/backups`, `/var/log`).
- Row 3 (two columns): PEER RX TX table · PID COMMAND CPU RSS top-4 processes.
- Unreachable/offline host: the metric area is replaced by a centred block — the state in its color,
  `last reading: … — not live`, and `N tmux session(s) still alive on the host · r reconnect` when applicable.
- Footer: `←→ other host · ⏎ connect · r reconnect · Esc back`, right `numbers only — no thresholds, no alerts`.
- Keys: `←→` (also `h l`, `j k`, `↑↓`) cycle hosts with wrap; `⏎` connect; `r` reconnect; `Esc` returns to
  the session if one is active, otherwise the list; `?` help; `q` quit.
- Status bar gains a `health` chip beside `hosts` (inverted cyan when showing).

### Switcher (`^A s`) — tmux `display-popup`

- Dims the pane behind it to 72% black. 560px, cyan border.
- Rows: `N host cmd status`; the current session shows `here` in green; unreachable shows
  `unreachable · tmux alive` in red in the command column.
- Selection (↑↓/jk) is separate from the active session; opens positioned on the active session.
- `1–9` jump immediately (no Enter). `⏎` switch to selection. `x` asks to kill the *selected* row.
  `n` closes and goes to the host list. `Esc` closes. Empty state: `no sessions yet`.

### Help (`?` in the list, `^A ?` in a session)

Full-frame overlay on `#080b0d` (not a popup). Content in `DESIGN.md`. `Esc`, `?` or `q` close it.

### Add / edit host (`a` / `e`)

- 480px cyan-bordered popup, title `add host` or `edit host`, right hint `⇥ next field`.
- Three fields — `name`, `user`, `address` — each a bordered box; the active field has a cyan border and a
  blinking caret. `Tab` cycles, `⏎` saves, `Esc` cancels.
- Validation: `name` and `address` required → toast `name and address are required`. Empty user → `root`.
- Add appends and selects the new host; edit updates in place. Toast `host added` / `host updated`.
- Footer hint: add → `essh will try the host on save`; edit → `changes apply to new connections`.

### Kill confirmation (`^A x`, or `x` in the switcher)

420px popup with a **red** border. `Kill session N on <host>?` then
`<cmd> will be terminated. Detach (^A d) instead to keep it running.` Keys: `y` yes, `Esc`/`n` no.
After a kill: if it was the active session, activate the nearest remaining one or return to the host list if
none remain. Toast `session killed`.

### Quit (`q` in the host list)

Whole frame becomes the local shell on `#050708`:
`[detached from essh]` · `N tmux sessions still running on M hosts. Run essh to come back.` · a local prompt
with a grey blinking cursor · bottom-left `press any key to relaunch essh`. Any key returns to the host list
with all state intact. **Nothing is killed on quit.**

## Key map (complete — 12 keys plus tmux)

Prefix is `Ctrl+A`. The prototype also accepts backtick `` ` `` as the prefix for browsers that eat Ctrl+A.
Arming the prefix shows a centered pill above the status bar: `^A — waiting for key: 1–9 s n m d x ?`.
Pressing the prefix again disarms it. Any other key after the prefix is consumed and does nothing.

Host list: `↑↓ j k` move · `⏎` connect / reattach · `/` filter (incremental on name and address; `⏎` connects
the first match, `Esc` clears, `Backspace` edits) · `a` add · `e` edit · `h` health of selected host · `?` help ·
`q` quit.

Session: keys go to the shell, except `Ctrl+L` clear, `Ctrl+C` prints `^C`, `Esc` clears the typed line, and the
prefix sequences: `^A 1–9` jump · `^A s` switcher · `^A n` new session on the host selected in the list
(toast `pick a reachable host first` and return to the list if that host is down) · `^A m` strip → half → off ·
`^A h` health of this host · `^A d` host list · `^A x` kill (confirm) · `^A ?` help.

Anything in a popup: `Esc` closes. Mouse: click a status-bar chip to switch; click `hosts` to go to the list.

## Status bar rules

- Left: `essh` · `hosts` chip · `health` chip (each inverted cyan when its screen is showing) · `│` · one chip
  per session `N host ●`.
  The chip list is **identical on every screen**; only the active highlight (bg `#10171b`, white, 2px cyan
  underline) moves. Dot color = host reachability.
- Right: `↓ rx ↑ tx` and `rtt Nms` for the **active session's host only** — omitted entirely when there is no
  reachable active session — then the clock `HH:MM`.
- The bar is one tmux row (26px in the reference). Overflow is clipped, never wrapped.

## Toasts

Top-right of the frame, `#0d1316` on a `#1c2429` border, 12px, auto-dismiss after ~1.8s. Used for:
`<host> is offline`, `no session N`, `pick a reachable host first`, `name and address are required`,
`host added`, `host updated`, `session killed`, `reconnected to <host>`.

## Graphs — normalisation rules (important)

- **Latency** sparklines normalise against a **fixed 0–50 ms ceiling**, never the buffer's own max. A steady
  4–8 ms host must render as a flat `⣀` baseline; only genuine jitter climbs.
- **rx/tx** sparklines normalise against the buffer max × 1.6 headroom so the trace sits in the low-to-mid
  glyphs; `⣿` appears only on real peaks.
- **cpu** history normalises against a fixed 0–100 ceiling.
- Glyph ramp is five steps: `⣀ ⣄ ⣤ ⣶ ⣿`. Buffers: 10 samples in the host list, 14 for rtt in the strip,
  27 for rx/tx in the strip, 26 for rx/tx in the half view, 44 for rx/tx and 24 for cpu on the health page.
  Pad the left with `⣀` until full.
- Bars (cpu/mem/disk) are flat green on `#14191d`, no color ramp, no thresholds.

## State model

```
screen:   hosts | session | health | quit
popup:    null | switch | help | form | confirm
sel:      index into hosts (list selection)
active:   index into sessions | null
strip:    0 | 1 | 2                    (off, 300px strip, half)
prefix:   bool
filter:   null | string
typed:    string                      (current command line)
swSel:    index (switcher selection)
form:     { edit: index|null, field: 0..2, values: [name,user,addr] } | null
confirm:  session index | null
hosts:    [{ name, user, addr, state: up|unreachable|offline, rtt, last }]
sessions: [{ host, cmd, age, lines: [[text, color]] }]
```

Derived, never stored: windows-per-host (count of sessions on that host), chips, switcher rows, the status
bar's right side, all sparklines.

## Tokens

See `DESIGN.md`. Summary: three semantic colors — cyan `#4fd1c5` (accent, keys, TX, selection, popup border),
green `#35d07f` (reachable, RX, bars, success), red `#ef5f52` (unreachable, kill confirm) — plus greys. The
prototype's fake shell uses `#e3bf4d` for one warning line (`tmux` inside tmux); that is the only yellow and
can be dropped. No gradients, no radii, no rounded borders. `JetBrains Mono` 400/700.

## Removed on purpose

Fleet consensus and configuration divergence, fleet-wide health dashboard and alerts, broadcast, activity log,
quick-connect, command palette, config screen, file browser, port forwards, btop numbered panels, gradient
meters, function keys. If files or forwards come back, they are tmux popups (`^A f`, `^A p`) in this skin —
never a screen. Ask before re-adding anything.
