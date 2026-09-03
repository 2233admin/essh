# Handoff: ESSH TUI — simplified redesign (v2)

## Overview

`essh` is a terminal UI for working across several SSH hosts: it keeps **persistent sessions** (they survive
detaching and quitting essh), lets you move between them, and shows **host health** so you can see which
machine is in trouble.

This is the **simplified** direction. An earlier pass (kept in `reference/` for comparison) centred on fleet
configuration divergence — consensus percentages, facet-by-facet agreement, per-host verdicts. That was too
advanced for the intended user. **v2 removes all of it.** What remains is the smallest set of screens that
answers three questions: *what sessions do I have*, *which hosts are healthy*, and *what's happening on the
one I'm in*.

Visual language stays **btop** (numbered rounded panels whose titles break the top border, gradient cell
meters, braille history graphs) plus **netwatch** (mirrored up/down network history).

Target implementation: the `essh` Rust TUI (ratatui-style widgets). The design reference is HTML but the
destination is a character grid — see "Translating to a TUI".

## About the Design Files

`ESSH Redesign v2.dc.html` is a **design reference created in HTML** — a prototype of look, layout and
hierarchy. It is not production code and should not be copied into the app. Recreate the design using the
target codebase's own patterns and widgets (ratatui `Block`, `Table`, `Sparkline`, `Gauge`).

Open it directly in a browser. Switch screens with the tab row above the terminal frame or the tabs inside
the header. A summary card under the frame lists what changed from v1.

`reference/ESSH Redesign v1.dc.html` is the **superseded** advanced version — useful only to see what was
deliberately dropped. Do not implement it.

## Fidelity

**High-fidelity.** Colors, severity semantics, column order and copy are final. Pixel geometry does not
transfer literally.

### Translating to a TUI

| HTML in the reference | TUI equivalent |
| --- | --- |
| Panel: 1px `#2a3540` border, `border-radius:6px`, title absolutely positioned over the top edge at `left:16px` | `Block::bordered()` + `BorderType::Rounded`, left-aligned title with one space padding; keep the leading panel number + name |
| Panel bottom-edge key hints | Block bottom title, left-aligned |
| Column widths in `ch` (e.g. `15ch`) | `Constraint::Length(15)` — the `ch` numbers ARE cell counts |
| Column widths in `px` (meters) | one cell ≈ 5px in the reference |
| `1fr` columns | `Constraint::Min(…)` / `Fill` |
| Gradient cell meter (striped gradient under a `repeating-linear-gradient` mask) | run of `█` cells, each colored by its **position along the full scale**, not by the value — btop-style ramp |
| Braille history string (`⣀⣠⣤⣶⣿`) | ratatui `Sparkline` with braille marker |
| Panel gaps 20–26px | 1 blank row / 2 blank columns |
| `font-size` differences (11–21px) | cannot transfer — re-express with **dim / normal / bold + color**. Big figures become `bold` |

Everything the reference says with size must be said with weight, dimming and position.

## Screens / Views

All six share one chrome: window frame → single-line global header → screen body → single-line footer.

### Global header (all screens)

One line. Left: `ESSH` cyan bold with `letter-spacing:.14em`, then four numbered tabs — `1 sessions`,
`2 hosts`, `3 health`, `4 config`. Active tab: `#cbd5dd` text, `#131a20` background, 2px cyan `#58d6d0` bottom
rule; inactive `#6b7c8a`. **Derive the active tab from current screen**, don't hard-code:
sessions → `1 sessions`; hosts → `2 hosts`; health → `3 health`; help → `4 config`; terminal and files also
map to `1 sessions`.

Right: three counted dots — `● 9 up` (green `#35d07f`), `● 1 busy` (yellow `#e3bf4d`), `● 1 offline`
(grey `#47555f`) — then `6 sessions` and the clock. No empty band under the tabs, no separate theme line.

### Footer (all screens)

One line, top border `#1a2129`, background `#0b0f13`, 11.5px, key glyphs `#8b9aa6` on labels `#47555f`:
`⏎ attach · n new · d detach · m monitor · / search · F1 help · ^P menu · q quit`, right-aligned
`theme dark · 150×34`.

---

### 1. Sessions — the landing screen

The app opens here. Sessions are the primary object; hosts are how you get more of them.

**Panel 1 — `sessions`** (`1 sessions │ 6 open on 5 hosts`; top-right `detached survive restarts`)

Grid `3ch 14px 13ch 9ch 11ch 8ch 7ch 1fr 9ch`, gap 14px, 12.5px, `white-space:nowrap`.
Headers: `#` · (dot) · HOST · STATE · RUNNING · UP · CPU · LAST LINE · IDLE.

- `#` — the session number you press to jump to it; cyan bold on the selected row
- dot — host reachability color
- STATE — `attached` green, `detached` `#5b6b7a` (normal and unalarming), `dropped` red
- RUNNING — the foreground command (`deploy.sh`, `pg_dump`, `cargo build`, `-bash`)
- CPU — that session's process tree, colored by severity (`61%` yellow, `94%` red, `—` when offline)
- **LAST LINE** — the session's most recent line of output, ellipsised. This is the key column: it tells you
  what a detached session is doing without attaching. For the dropped session it carries the recovery instead:
  `connection reset — r reconnect, scrollback kept`
- IDLE — time since last output (`now`, `4s`, `18m`)

Six rows: web-02 attached/deploy.sh (selected), db-01 pg_dump 61%, db-01 -bash idle 18m,
build-01 cargo build 94%, cache-01 redis-cli, edge-01 dropped.

**Selected row** is a nested sub-grid spanning all columns, repeating the same
`grid-template-columns`/gap so it stays exactly row-height: `background:#16212a`,
`box-shadow:inset 2px 0 0 #58d6d0` (cyan left marker that shifts no columns), `border-radius:2px`,
`margin:1px -8px`, `padding:3px 8px`. In the TUI: a row style plus `▌` in the gutter column.

Hints: `↑↓ select · ⏎ attach · d detach · n new · w close`.

**Bottom row, three panels (`1fr 1fr 1fr`):**

- **`2 quick connect`** — the hosts with *no* session yet, numbered `1–5`; press the number to open one.
  Right column reads `no session`, or `offline` dimmed. Footnote `Press the number to open a session.`
  (Hosts with sessions are absent by design — they're in the table above.)
- **`3 broadcast │ type once, many hosts`** — target chip (`tag:web`, `3 hosts`), a command input line on
  `#080b0e` with a cyan block caret, then per-host results: `web-01 ok / exit 0 · 40ms`, `web-03 … / running`.
  This is the only multi-host *action* in v2, and it replaces all the fleet config tooling.
  Hints `b broadcast · t change target`.
- **`4 activity │ last 15m`** — five timestamped lines, grid `9ch 1fr`, host name in its severity color:
  session dropped, cpu over 90% for 4m, pg_dump started, attached, went offline. Plain sentences.

---

### 2. Hosts — a health list

One full-width panel. Same table shape as v1's host list, but **the columns are health, not configuration**.

**`1 hosts │ 11 hosts · refreshed 4s ago`**; top-right `sort load ↓`.

Grid `14px 15ch 14ch 6ch 8ch 9ch 9ch 6ch 12ch 1fr`, gap 14px, 12.5px, `white-space:nowrap`.
Headers: (dot) NAME ADDRESS SESS CPU MEM `DISK /` RTT `RTT 60s` TAGS.

- SESS — session count in cyan, `·` when none, red when the host's session is dropped
- CPU / MEM / DISK — each a `1fr 4ch` sub-grid: 8px cell meter + right-aligned percentage.
  CPU uses the utilisation ramp `#35d07f → #e3bf4d 60% → #ef5f52`; MEM is flat blue `#5aa9f0`;
  DISK is flat green, switching to `#e3bf4d` past ~80%. The **number** takes the severity color
- RTT — milliseconds, plus an 11-char braille sparkline (`RTT 60s`)
- TAGS — short and flat (`prod`, `web`) — no `env=` / `region=` prefixes, no `+1` overflow, since grouping
  no longer drives anything

Eleven rows; web-02 selected (same nested sub-grid treatment). **Unreachable hosts replace their meter cells
with one spanning explanation** (`grid-column:5/10`) rather than leaving five blanks:
`edge-01 → connection reset 6m ago · last seen cpu 9%, mem 41% · r reconnect` in red;
`staging-web-01 → offline since 12:10 · no response on 2222 · r retry` in grey.

Hints: `↑↓ select · ⏎ connect · m monitor · a add · e edit · / search`.

---

### 3. Health

**Panel 1 — `1 needs attention │ 3 of 11 hosts`**

The most important panel in v2: it says in words which hosts to look at, and *tells you when it's fine*.
Three rows, grid `12ch 9ch 1fr`: host (severity color, 500 weight), short verdict, then a sentence with the
cause and the reassurance:

- `edge-01 · offline · connection reset 6m ago, 1 session waiting to reconnect · scrollback preserved`
- `build-01 · cpu 94% · over 90% for 4m — cargo build in session 4 · expected during a build`
- `db-01 · disk 81% · / has 71 GB free, filling ~4 GB/day · pg_dump writing to /backups`

Attributing load to the session causing it is the point — a busy host with a known cause is not an incident.
Hints `⏎ open host · r reconnect`.

**Host cards — 4-column grid, only the hosts that need watching**

Each card: title strip with severity dot + host name + session count; a 21px bold cpu figure with `load` on the
right; an 18-char braille history; then three 11px rows — `mem` blue meter, `disk` green/yellow meter, `net`
as `↓` magenta `#b487f0` and `↑` cyan; footer line `up 12d · deploy.sh running`.
db-01's card has the focused border `#2f4652`.

**The offline card is styled as stale, not broken:** dimmer background `#0b0e12`, plain border `#232c34`,
`—` instead of a figure, a dead `#2f3a43` sparkline, and the explicit disclaimer
`Connection reset 6m ago. Values are the last reading, not live.` plus `r reconnect · s silence 1h`.
Never render stale numbers as if live.

**Bottom row, two panels:**

- **`2 busiest right now │ across all hosts`** — grid `11ch 1fr 5ch 6ch`: HOST PROCESS CPU RSS. Five rows
  spanning hosts (rustc 88.2 red, `postgres: pg_dump` 52.4 yellow, …). One list answers "what's eating the
  fleet" without opening any host. Hints `⏎ open session · s sort`.
- **`3 reachability │ rtt, last 10m`** — grid `12ch 1fr 6ch`, one row per host: name, 20-char braille rtt
  history, current ms. Colors: green healthy, yellow jittery (build-01), red for the dropped host with `×`,
  dead grey for the offline one.

---

### 4. Terminal + strip

Session chips row: `SESSIONS` label, then six chips `N host ●` (dot = host severity, so trouble is visible
without leaving the session) — active chip `#16212a` / `#fff` / border `#2f4652`; then a dashed `+ new`.

Body grid `1fr 330px`:

- **Terminal pane** — `#080b0e`, 12.5px, `line-height:1.62`, min-height 520px. One 11.5px meta line:
  `web-02 · matt@127.0.0.12:2222 │ up 8m │ rtt 0.4ms` with right-aligned `2 forwards` yellow. Prompt colors:
  user@host green `#35d07f`, `:` `#47555f`, path blue `#5aa9f0`, `$` `#47555f`, command `#8b9aa6`,
  output `#cbd5dd`, success `✓` green. Cursor is a cyan block.
- **Strip** — four small panels with the same chrome at 11.5px titles: `1 cpu` (figure + braille),
  `2 memory` (figure + bar), `3 net` (mirrored ↓/↑ braille, upload `transform:scaleY(-1)`),
  `4 top` (grid `1fr 4ch 7ch`, identical siblings collapsed as `nginx: worker ×6`). Below the strip, one
  dim line: `F5 hide strip · m full monitor`.

---

### 5. Files + forwards

- **`1 local`** — focused border `#2f4652`; path in the title **pre-shortened LTR** (`…/essh-demo/home/Downloads`).
  Do **not** truncate paths with an RTL/`direction` trick: it reorders the neutral `/` characters and displays
  a path that isn't real.
- **`2 remote │ web-02 : /home/matt`** — grid `14px 1fr 10ch`: glyph, name, size. Directories blue `#5aa9f0`
  with `▸` and an item count instead of a size. The file that exists in both panes (`app-2.14.4.tar.gz`) is
  yellow on both sides to flag the collision.
  Hints `d download ← · m mkdir · Del delete · ⇥ switch pane`; local `↑↓ select · ⏎ open · u upload →`.
- **`3 transfer │ 1 active`** — grid `1fr 240px 20ch`: label, 11px green meter at 52%, and
  `52% · 3.1 MB/s · 6s`. Always show rate and ETA, not just a bar.
- **`4 port forwards │ web-02 · 2 rules`** — grid `4ch 22ch 20ch 12ch 1fr`: DIR BIND TARGET STATE LAST CHECK.
  State is **probed, never assumed**: `● failed` red with the reason
  `no listener on 8080 — bind refused, 2s ago · r retry`, and `● active` green with
  `probed 2s ago · 1 open connection`. The add-forward format sits in a dashed row **outside** the table so it
  cannot clip: `a  add forward  L 127.0.0.1:PORT → HOST:PORT │ R HOST:PORT → 127.0.0.1:PORT`.

---

### 6. Help

Grid `1.25fr 1fr`.

- **`1 keys │ everything on one screen`** — lead line
  `In the dashboard, single keys. Inside a session, Ctrl+A first — release, then the key.`
  Then **six** function keys in a 3-column grid (F1 help, F2 monitor, F3 files, F4 forwards, F5 mini strip,
  F9 new session — down from ten in v1). Then two labelled columns, IN A SESSION and DASHBOARD, each a stack
  of grid `11ch 1fr` rows. **Every binding is its own grid cell**, so no label wraps mid-phrase.
  `q quit (sessions keep running)` states the persistence guarantee at the point of anxiety.
  Hints `/ search bindings · Esc close`.
- **Command palette** (floating card, border `#2f4652`, background `#0e141a`, shadow) — query row `❯ db` with
  a cyan block caret and right-aligned `5 of 24`. **Results are sessions and hosts, not settings**: attach an
  existing session (with its running command as the right-hand hint), start a new one, or monitor a host (with
  its warning `disk 81%` as the hint). Active row `#16212a` + 2px cyan left border + `#fff`.
  Footer `⏎ run · ↑↓ select · Esc dismiss`.
- **`2 config │ ~/.essh`** — grid `11ch 1fr`: `hosts` (config.toml, 11 hosts), `sessions`
  (`6 running · survive quitting essh`), `refresh` (`5s · health poll interval`), `theme`.
  Note `Edits to config.toml reload live — no restart.` Hints `e edit · t cycle theme · o reveal`.

## Interactions & Behavior

- **Screen switching:** header tabs and the demo tab row call the same navigation; the active tab is computed
  from screen state. Prev/next arrows cycle sessions → hosts → health → terminal → files → help.
- **Sessions are persistent.** Detaching keeps the remote command running; quitting essh keeps sessions
  running; a dropped connection preserves scrollback and offers `r` to reconnect into the same session. This is
  a behavioral requirement, and the UI states it in three places (panel subtitle, `q quit` hint, config panel).
- **Health polling** every 5s (configurable) drives dots, meters, sparklines and the `refreshed 4s ago` label.
  RTT history is a rolling 60s buffer per host; the health screen's reachability graph is 10m.
- **Attention rules:** a host enters "needs attention" on offline/dropped, cpu >90% sustained ~2m+, mem >90%,
  or disk >80%. Each entry must name the **cause** (which session/process) when one is known, and must say when
  the condition is expected. Sustained thresholds only — no flapping on a single sample.
- **Stale data is labelled, never silently shown.** Unreachable hosts show `—`, dim their card, and say the
  values are the last reading. Never a `0` for unknown.
- **`r`** is uniformly reconnect/retry — on a dropped session, an offline host row, an offline card, and a
  failed forward.
- **Broadcast** targets a tag, streams per-host exit code and duration, and never blocks the UI.
- **Process grouping:** identical sibling commands collapse with `×N` and summed RSS.
- **Hover** (pointer contexts only; drop for the TUI): inactive tabs lighten `#6b7c8a` → `#cbd5dd`, borders
  `#232c34` → `#3a4a56`. No transitions.
- **Copy rules:** correct pluralisation, no `" ago"` under a column header that already says it, no duplicated
  figures, and nothing that asks the user to interpret a raw ratio.

## State Management

- `screen` — `sessions | hosts | health | terminal | files | help`; drives body and derived `activeTab`
  (`sessions | hosts | health | config`).
- **Sessions:** id/number, host, attached|detached|dropped, foreground command, started-at, cpu% of process
  tree, last output line, last-output-at, scrollback buffer, forwards count.
- **Hosts:** name, address, port, user, tags, reachable state, sessionCount, cpu%, mem%, disk% (per mount),
  load, rtt current + 60s history, lastSeen, offline reason.
- **Health:** derived attention list (host, kind, threshold crossed, duration, attributed session/process,
  expected flag); fleet-wide process list (host, command, cpu, rss); 10m rtt histories.
- **Monitor samples** for the focused host: cpu total + history, memory breakdown, net per-flow + up/down
  history, top processes.
- **Files:** local + remote listings, focused pane, selection, transfer queue with progress/rate/ETA.
- **Forwards:** rule list with probed state, reason, connection count, last-check timestamp.
- **Broadcast:** target tag, command, per-host status/exit/duration.
- Timers: 5s health poll, per-forward verification, rtt ping.

## Design Tokens

| Token | Hex | Use |
| --- | --- | --- |
| ground | `#07090b` | page behind the window |
| window | `#0a0d10` | terminal ground; strip behind panel titles |
| panel | `#0c1014` | panel fill |
| panel-stale | `#0b0e12` | offline host card |
| panel-alt | `#0e1216` / `#0e141a` | title bar, palette card |
| terminal | `#080b0e` | scrollback, command input |
| footer | `#0b0f13` | footer bar |
| row-selected | `#16212a` | selected row, active chip, active palette row |
| meter-track | `#161d24` (`#1b2731` on selected rows) | empty meter cells |
| border | `#232c34` | default |
| border-strong | `#2a3540` | panel |
| border-focus | `#2f4652` | focused pane, palette card |
| rule | `#1a2129` | hairlines |
| text | `#cbd5dd` | primary |
| text-strong | `#ffffff` | selected row |
| text-mid | `#8b9aa6` | secondary |
| text-dim | `#5b6b7a` | labels, headers |
| text-dimmer | `#47555f` | hints, offline |
| text-faint | `#3a4750` / `#2f3a43` | separators, dead sparkline |
| cyan (chrome) | `#58d6d0` | panel numbers, keys, session numbers, focus, caret — **never a measurement** |
| green (ok) | `#35d07f` | up, attached, healthy, transfer |
| yellow (busy) | `#e3bf4d` | busy, warning threshold |
| red (bad) | `#ef5f52` | offline, dropped, failed, >90% |
| blue (memory) | `#5aa9f0` | memory meters, directories |
| magenta (net down) | `#b487f0` | download |

Utilisation ramp: `linear-gradient(90deg,#35d07f,#e3bf4d 60%,#ef5f52)`, `background-size` set to the meter's
full width so the color reflects scale position, not fill length. Cell mask:
`repeating-linear-gradient(90deg,#000 0 3px,transparent 3px 5px)` on 7–8px meters, `0 4px/4px 6px` on 9px,
`0 5px/5px 7px` on 11px.

Type: `JetBrains Mono` 400/500/700 (fallback `ui-monospace, monospace`). Scale: 21px card figures,
17px strip figures, 15–16px braille, 13px header, 12.5px body/tables, 12px panel titles, 11.5px hints and
dense rows, 11px card internals. `letter-spacing:.1em` on uppercase headers and panel names, `.14em` on
`ESSH`, `-1px` on braille runs, `line-height:1.62` in the terminal.

Spacing: panel padding `16px 14px 12px` (cards `15px 13px 12px`); panel gaps 20–26px; grid column gaps
12–16px; row gaps 4–9px. Panel titles offset `top:-9px; left:16px` with `padding:0 9px` in the window color;
bottom hints `bottom:-9px`. Radii: 10px window, 6px panel, 4px chips, 2px rows and meters.
Shadows: `0 30px 80px -30px #000` (window), `0 20px 50px -20px #000` (palette).

## Assets

None. No images, no icon fonts. Glyphs are Unicode and must exist in the terminal font:
`● ◌ ◔ ▸ ↰ ↑ ↓ ← → ⏎ ⇥ ✓ × · │ ❯ +` and braille `⣀ ⣄ ⣠ ⣤ ⣶ ⣿`. Braille runs in the reference are sample data;
the app generates them from real history buffers.

## Files

- `ESSH Redesign v2.dc.html` — **the design to build.** Six screens plus a "what changed from v1" card.
- `reference/ESSH Redesign v1.dc.html` — superseded advanced version (fleet consensus/divergence). Reference only.
- `reference/original-screenshots-README.txt` — v0.3.3 screen tour and the author's bug list.

## Carried-over defect fixes

These were found in v0.3.3 and remain fixed in v2:

1. Port forwards reported `Active` with nothing bound — state is probed; failures carry a reason and a retry key.
2. `r probe` reloaded the host list and discarded probe state — `r` is now uniformly reconnect/retry, and
   config reload never drops live health data.
3. The add-forward format hint clipped at the panel edge — now its own row outside the border.
4. Help text wrapped mid-phrase (`F4 / forwards`) — the keymap is a grid.
5. Bad pluralisation (`1 peers`) and unknown values shown as zero — counts are pluralised; unknown is `—` or `×`.
6. Dead vertical band under the tabs, a redundant `Theme: dark` echo line, and ~300px of empty ground per
   screen — header is one line, theme appears once in the footer, content fills the frame.
7. Eleven of fifteen process rows spent on identical nginx workers — siblings collapse with `×N`.

## Deliberately dropped in v2

Fleet consensus scoring, the `DIVERGE` column, tag/group agreement bars, per-facet percentages, host verdicts
with config hashes, and the `p`/`P`/`R` probe-vs-reload key split. If configuration comparison is ever wanted
back, it belongs behind an explicit command (`essh diff <tag>`), not on the default dashboard.

## Open / not covered

Add-host and edit-host forms, the search/filter overlay, the full-screen monitor for a single host (v1's
monitor screen is the model — see `reference/`), and theme variants beyond `dark`. Each theme needs the same
semantic roles remapped: ground, panel, border, four text greys, chrome cyan, and the four severity hues;
a light theme needs the severity hues darkened for contrast.
