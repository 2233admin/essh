# Handoff: essh v3 — tmux-based multi-session SSH with a netwatch skin

## Overview

essh v3 is deliberately small. **tmux does the work**: every essh session is a tmux window on the remote host,
so detach, reconnect-after-drop, and keep-running-after-quit are tmux behaviors, not essh features. essh adds
exactly three things on top:

1. a **host list** (the launch screen),
2. a **session switcher** (a tmux `display-popup`),
3. a **skinned tmux status bar** plus an optional **netwatch-style side strip**.

Everything else from earlier passes (fleet health dashboards, broadcast, palette, file browser, port forwards,
config screen) is removed. See "Removed" at the end. Do not re-add without asking.

Target: Rust TUI for the host list and popup (ratatui), `tmux.conf` for the status bar, a small shell/Rust
helper for the strip's metrics. The design reference is HTML; the destination is a character grid.

## About the Design File

`ESSH v3 tmux.dc.html` is a **design reference created in HTML** — a prototype of look, layout and hierarchy,
not production code. Recreate it with the target's own tools. Open it in a browser; the four buttons above the
frame switch between hosts / session / switcher / help. Two note cards below the frame summarise intent and
removals.

## Fidelity

**High-fidelity** for palette, semantics, column order and copy. Pixel geometry does not transfer — use the
`ch` widths as cell counts, and express size differences with bold/dim/color.

### Translating to a TUI / tmux

| Reference | Implementation |
| --- | --- |
| 1px `#1c2429` square border | `Block::bordered()` with `BorderType::Plain` (netwatch uses straight corners; **no rounded borders**) |
| Column widths in `ch` | `Constraint::Length(n)` exactly |
| `1fr` column | `Constraint::Fill` |
| Braille strings `⣀⣠⣤⣶⣿` | ratatui `Sparkline` (braille) or generated from history buffers |
| Flat 6px bars (`#35d07f` on `#14191d`) | `Gauge` or a run of `█`/`░`; **single color, no gradient ramp** |
| Selected row: `background:#10171b`, `box-shadow:inset 2px 0 0 #4fd1c5` | row style bg `#10171b` + `▌` in cyan in the gutter column |
| Status bar (26px, `#0d1316`, top border) | tmux `status-style bg=#0d1316`, `status-left`/`status-right`, `window-status-format` — see below |
| Switcher popup (560px, cyan border, centered) | `tmux display-popup -E -w 70 -h 12 -b single -S fg=#4fd1c5` running the essh switcher binary |
| Side strip (300px, left border) | a tmux vertical split pane, 38 cols, running `essh strip <host>`; toggled with `^A m` |

## Screens

### Status bar (every screen — this IS the tmux status line)

Left to right, 12px, on `#0d1316` with a `#1c2429` top rule:

- `essh` cyan `#4fd1c5` bold
- `hosts` — a chip: inverted (cyan bg, `#080b0d` text) when the host list is showing, otherwise dim `#5b6b7a`
- `│` separator
- one chip per session: `N host ●`. Number + host in `#8b9aa6`; the dot carries **host reachability**
  (green `#35d07f`, red `#ef5f52`). The active session chip: bg `#10171b`, white text, 2px cyan bottom rule.
  **The session list is identical on every screen** — only the highlight moves.
- Right side, `#5b6b7a`: `↓ 8.4M ↑ 1.2M` (arrows green/cyan), `rtt 4ms`, clock `12:25` in `#8b9aa6`.
  These are the **current host's** rx/tx/rtt. Nothing else goes here.

tmux mapping: `status-left` = `essh` + hosts chip; `window-status-format` = `#I #{@essh_host} #{@essh_dot}`;
`window-status-current-format` = same with the active style; `status-right` = output of `essh statusline`.

---

### 1. Hosts (launch screen)

Header line: `essh │ 10 hosts · 4 sessions`, right `/ filter` in `#5b6b7a`.

Grid `14px 16ch 24ch 5ch 16ch 1fr`, column gap 16px, rows tight (no row gap), `white-space:nowrap`.
Headers in `#5b6b7a`: (dot) HOST USER@ADDRESS TMUX LATENCY LAST, then a 1px `#14191d` rule.

Per row:

- dot: `●` green reachable · `●` red unreachable-but-session-alive · `○` `#3f4b52` offline
- HOST `#cbd5dd`; USER@ADDRESS `#5b6b7a`
- TMUX — number of tmux windows open on that host, cyan; `·` `#3f4b52` when none; red when the host is
  unreachable but windows exist
- LATENCY — 10-char green braille + `4ms` at 12px. Unreachable rows replace this with `unreachable 6m` in red;
  offline rows with `offline` in `#3f4b52`
- LAST — `2d ago` style for idle hosts; for hosts with sessions, what's running (`attached · deploy.sh`,
  `pg_dump · 42m`); for unreachable, `tmux still running · ⏎ reconnect`

Selected row (web-02): sub-grid spanning all columns with the same template, bg `#10171b`, cyan inset left
marker, `margin:0 -10px; padding:1px 10px`, host name white bold.

Ten hosts in the reference. Footer hint line, `#3f4b52` with keys in `#8b9aa6`:
`⏎ connect · a add host · e edit · / filter · ? help · q quit`, right-aligned
`TMUX = windows open on that host · sessions keep running after quit`.

**Enter** on a host with windows reattaches; without, creates a window and attaches. Both are one key.

---

### 2. Session

Grid `1fr 300px`. Left: the tmux pane, 13px, `line-height:1.6`, on `#080b0d`. Prompt colors: user@host green,
`:` and `$` `#3f4b52`, path cyan, command `#8b9aa6`, output `#cbd5dd`, log lines `#8b9aa6`, `✓` green. Cursor is
a cyan block. Nothing is drawn over the pane — no meta line, no chips.

Right: the **netwatch strip**, `border-left:1px solid #1c2429`, padding `10px 14px`, 12px, sections
separated by 1px `#14191d` rules, 14px apart:

1. `web-02` cyan bold, right `eth0` dim
2. **RX** — label `#5b6b7a`, value `8.4 MB/s` green; 27-char green braille at 15px
3. **TX** — value `1.2 MB/s` cyan; cyan braille (not mirrored — netwatch stacks them)
4. Grid `5ch 1fr 5ch`: `cpu` / `mem` / `disk` flat green 6px bars with right-aligned %, then `rtt` with a
   14-char braille and `4ms`
5. Peer table, grid `1fr 6ch 6ch`: PEER RX TX, four rows, peers `#cbd5dd`, numbers `#8b9aa6`
6. Bottom: `^A m hide` in `#3f4b52`/`#8b9aa6`

The strip is off by default; `^A m` toggles it. It is a tmux pane, so it survives switching sessions and
follows the active host.

---

### 3. Switcher (`^A s`)

The session pane dims to `#3f4b52` behind a centered 560px popup: 1px **cyan** border, `#080b0d` fill,
`0 20px 60px -10px #000` shadow.

- Title row: `switch` cyan bold, right `^A s` dim; 1px `#1c2429` rule
- Rows, grid `4ch 14ch 1fr 8ch`, padding `3px 14px`: number (cyan), host, what's running (`#8b9aa6`),
  right-aligned status. Active row: bg `#10171b`, cyan inset marker, white host, `here` in green.
  Unreachable: running column reads `unreachable · tmux alive` in red.
- A rule, then `n new… pick a host` dim
- Footer: `1–9 jump · ⏎ switch · x kill · Esc close`

`1–9` jump instantly without confirming. This popup is `tmux display-popup` running `essh switch`.

---

### 4. Help (`?` in list, `^A ?` in session)

Title `essh keys` cyan bold; subtitle `essh is tmux underneath. Every tmux key still works; these are the ones
you need. Prefix is Ctrl+A.` Three columns, 12.5px, key cells `8ch` cyan, labels `#8b9aa6`:

HOST LIST — `↑↓ jk` move · `⏎` connect (or reattach) · `/` filter · `a` add host · `e` edit host ·
`q` quit — sessions keep running

IN A SESSION — `^A 1–9` jump to session · `^A s` switcher · `^A n` new session on a host ·
`^A m` toggle netwatch strip · `^A d` back to host list · `^A x` kill this session

STATUS BAR — legend for the three dot states, and `Right side: current host's rx/tx and rtt. Nothing else.`

Twelve keys total. Footer `Esc close`.

## Behavior

- **Sessions are tmux windows** in one tmux server per remote host (`essh` names them `essh`). Attaching runs
  `ssh -t host tmux new -A -s essh`. Quitting essh detaches; nothing is killed unless the user presses `^A x`.
- **Reconnect** after a drop: the host list shows the red dot + `tmux still running`; Enter reattaches to the
  same window with scrollback intact. The switcher shows the same session as `unreachable · tmux alive`.
- **Reachability** is a ping/TCP-connect to port 22 every 5s; latency history is a rolling 10-sample buffer per
  host. Three states only: reachable, unreachable-with-sessions, offline. No thresholds, no verdicts.
- **Strip metrics** are read over the existing SSH connection (`/proc/net/dev`, `/proc/stat`, `free`, `df`,
  `ss -tn`) once per second while the strip is visible; nothing runs when hidden.
- **Status bar right side** refreshes with the strip data when the strip is on, otherwise every 5s from a
  lighter probe (rx/tx delta + rtt only).
- **`/` filter** is incremental on host name and tags; Esc clears.
- No mouse dependency; hover states in the reference are browser-only.

## Design Tokens (netwatch skin)

| Token | Hex | Use |
| --- | --- | --- |
| ground | `#050708` | behind the terminal |
| terminal | `#080b0d` | all pane and popup fills |
| status | `#0d1316` | tmux status bar |
| row-selected | `#10171b` | selected row, active chip |
| bar-track | `#14191d` | empty bar, hairlines |
| border | `#1c2429` | all borders and the status-bar rule |
| text | `#cbd5dd` | primary |
| text-strong | `#ffffff` | selected host |
| text-mid | `#8b9aa6` | secondary, key glyphs |
| text-dim | `#5b6b7a` | headers, labels |
| text-faint | `#3f4b52` | hints, offline |
| cyan | `#4fd1c5` | the one accent: brand, keys, TX, selection marker, popup border |
| green | `#35d07f` | reachable, RX, bars, success |
| red | `#ef5f52` | unreachable only |

No yellow, no orange, no blue, no magenta, no gradients. Three semantic colors and greys.

Type: `JetBrains Mono` 400/700 (fallback `ui-monospace, monospace`). 13px base, 12px strip/status/hints,
12.5px help, 15px braille in the strip, 11px `^A m hide`. `letter-spacing:-1px` on braille runs.
Bold only for the `essh` wordmark, popup title, and the selected host.

Spacing: screen padding `14px 18px`; strip padding `10px 14px`; grid column gap 16px (14px in the popup);
section gap 14px in the strip; status bar height 26px (one tmux row). No radii anywhere.

## Assets

None. Glyphs: `● ○ ↓ ↑ → ⏎ ✓ · │ █` and braille `⣀ ⣄ ⣠ ⣤ ⣶ ⣿`. Braille in the reference is sample data.

## Files

- `ESSH v3 tmux.dc.html` — the design to build. Four states plus two note cards.

## Removed (vs v1 and v2 — do not re-add without asking)

Fleet consensus / configuration divergence; health dashboard with "needs attention", host cards, busiest
processes, reachability panel; broadcast; activity log; quick-connect; command palette; config screen; file
browser; port forwards; btop numbered panels and gradient meters; function keys F1–F10.

If files or forwards are wanted later, the intended shape is a tmux popup (`^A f`, `^A p`) in this same skin —
not a screen.
