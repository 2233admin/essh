# ESSH TUI redesign — corrected handoff

Supersedes `design_handoff_essh_tui_redesign/README.md` (2026-09-03). The design
work in that document stands. This one fixes its arithmetic, resolves its
conflicts with the code it targets, and fills in the sections a TUI needs and a
web reference does not.

Read `ESSH Redesign.dc.html` for the look. Read this for the numbers, the
widget mapping and the rules.

## What changed from the original handoff

| # | Original | Corrected | Why |
| --- | --- | --- | --- |
| 1 | Header `● 1 unprobed`; hosts table lists 2 | 2 everywhere | The table and `role=web` both say 2; the summaries said 1 |
| 2 | "sixteen 100% facets" | fourteen | 7 comparable hosts × 18 facets = 126 checks. 20 × 7 = 140 |
| 3 | Header dots sum to 16 over 10 hosts | Two axes, stated separately | `reachable` and `diverged` overlap; the reader adds them |
| 4 | Verdict "5 of 10 hosts" under an `env=prod` panel | "5 of the 7 comparable hosts" | Panel 1 is scoped to `env=prod`; panel 2 was fleet-wide |
| 5 | Outliers panel: 6 rows | 5 rows | Only 5 hosts diverge |
| 6 | `role=db` `1 / 2` | `—  no majority` | 2 hosts, 2 values: the winner was decided by string order |
| 7 | CPU 11.5%, `load 4.64 4.31 5.14` | `load 0.92 0.81 0.74` | Load 4.64 on 8 cores is ~58% busy, not 11.5% |
| 8 | Net flows don't sum to the headline | They sum | 8.3 MB/s down, 1.2 MB/s up |
| 9 | `Block::bordered()` | `design::panel` | The repo already ported netwatch's panel idiom |
| 10 | 24 hex tokens | `theme::Theme` slots + 6 new ones | The theme contract is semantic and already exists |
| 11 | Six defects | Seven | The author's list has seven; the functional one was dropped |
| 12 | No minimum size | §7 | It is a TUI; the window is whatever the user made it |
| 13 | No `terminal` theme story | §6.3 | That theme switches the gradient ramps off by design |

## 1. Two probe states, never one word

The original uses "unprobed" for two independent facts. They come from
different mechanisms on different clocks, and conflating them is what produced
corrections 1 and 3.

- **Reachability** — a TCP connect every 60s (`fleet::FleetProber`). Yields
  `online`, an RTT, and the sparkline history. Answers *is it up*.
- **Facet collection** — an SSH session that reads configuration
  (`divergence::HostFacts`). Answers *what is on it*.

A host can be reachable with no facets (added a minute ago), and have facets
while unreachable right now (probed earlier, down since). Render them
separately: the status dot and RTT sparkline carry reachability, the DIVERGE
column carries facet state.

Facet state is a three-way partition of the fleet, so the header's counts add
up to the host count:

```
● 3 at consensus   ● 5 diverged   ◌ 2 unprobed          9/10 reachable
```

A host with no facets shows `◌` in DIVERGE, the word `unprobed`, and is
excluded from every agreement count with the exclusion stated in words. Never
`0`, never counted as agreement.

## 2. The canonical fixture

Every number on every screen derives from this table. The reference HTML, the
demo fleet and the render tests use it, so a number that disagrees with another
screen is a bug rather than a judgement call.

| host | env | region | role | reachable | facets | diverges on |
| --- | --- | --- | --- | --- | --- | --- |
| web-01 | prod | us-east | web | yes | **no** | — |
| web-02 | prod | us-east | web | yes | yes | listening ports |
| web-03 | prod | us-east | web | yes | yes | listening ports |
| db-01 | prod | us-east | db | yes | yes | — |
| db-02 | prod | us-east | db | yes | yes | /etc/nsswitch.conf |
| cache-01 | prod | us-west | cache | yes | yes | listening ports |
| edge-01 | prod | us-east | edge | yes | yes | timezone, /etc/ssh/sshd_config |
| monitor-01 | prod | us-west | monitor | yes | yes | — |
| build-01 | staging | us-west | build | yes | yes | — |
| staging-web-01 | staging | us-east | web | **no** | **no** | — |

Fleet-wide: 3 at consensus, 5 diverged, 2 unprobed, 9/10 reachable.

### 2.1 Fleet consensus, scoped to `env=prod`

8 members, 1 unprobed (web-01), **7 comparable**. 18 facets × 7 = **126
facet-checks**.

| facet | agree | % | colour |
| --- | --- | --- | --- |
| listening ports | 4 / 7 | 57% | orange |
| /etc/nsswitch.conf | 6 / 7 | 86% | yellow |
| /etc/ssh/sshd_config | 6 / 7 | 86% | yellow |
| timezone | 6 / 7 | 86% | yellow |
| *14 others* | 7 / 7 | 100% | green, no meter |

Disagreements 3 + 1 + 1 + 1 = 6. Agreement **120 of 126 = 95.2%**.

Headline: `95.2` `% agree`, `120 of 126 facet-checks`. Notes:
`4 facets diverge across 5 hosts` and
`1 host unprobed — facets unknown, not counted as agreement`.

### 2.2 Groups panel

Each row is computed inside its own peer set, which is why `role=web` reaches
full consensus while its members diverge under `env=prod`. Say so in the
panel's subtitle: `consensus is per tag, not fleet-wide`.

| group | hosts | at consensus | note |
| --- | --- | --- | --- |
| env=prod | 8 | 2 / 7 | edge-01 breaks timezone + sshd_config, and 4 other hosts |
| region=us-east | 7 | 1 / 5 | edge-01 breaks timezone + sshd_config, and 3 other hosts |
| region=us-west | 3 | 2 / 3 | cache-01 breaks listening ports |
| role=web | 4 | 2 / 2 | full consensus · 2 unprobed, excluded |
| role=db | 2 | — | no majority — 2 hosts hold 2 distinct values |

The ratio appears **once per row**, in the AT CONSENSUS column. The note says
what the ratio cannot.

### 2.3 No majority

A group needs a strict plurality before any host in it can be called wrong.
Two hosts holding two values is a disagreement with no consensus: show `—` in
the ratio column, `no majority — N hosts hold M distinct values` in the note,
and flag neither host as the outlier. A group with fewer than two probed hosts
shows `1 host probed — nothing to compare it against`.

Implemented in `divergence::score_categorical` and `GroupSummary::has_majority`.

### 2.4 Verdict and outliers

Both scoped to `env=prod`, and both say so.

Verdict lead: `5 of the 7 comparable hosts hold every outlier · 1 unprobed, excluded`.
Five entries: edge-01, web-02, web-03, db-02, cache-01.

Outliers: **5 rows** by divergence — edge-01 Δ2, then web-02, web-03, db-02,
cache-01 at Δ1. Summary: `2 hosts at full consensus · 1 unprobed, excluded`
(5 + 2 + 1 = 8, the group's size).

### 2.5 Monitor, on web-02

CPU `11.5%` of 8 cores, so `load 0.92 0.81 0.74`. `peers 15%` is the fleet
median (§5.2).

Memory `30.5 GB / 66.7 GB` = **46%**.

Net `↓ 8.3 MB/s` `↑ 1.2 MB/s`, and the flow table sums to it:

| peer | proc | ↓ | ↑ |
| --- | --- | --- | --- |
| 10.0.2.14:5432 | postgres | 5.9M | 412k |
| 10.0.2.9:6379 | redis-cli | 1.4M | 206k |
| 10.0.3.2:443 | nginx | 802k | 540k |
| 127.0.0.1:2222 | sshd: matt | 218k | 61k |
| 10.0.0.1:53 | systemd-resolve | 12k | 3k |

15 processes, 6 nginx workers collapsed to one `×6` row → 10 rows displayed.

Transfer: `app-2.14.4.tar.gz`, 38.8 MB, `52% · 3.1 MB/s · 6s`.

## 3. Panels

Use `design::panel::pane` with `PanelOpts`. It is netwatch's idiom, already
ported, and it carries everything the reference draws on the border:

```
╭─┤1├─┤ hosts ├─┤ 10 hosts · 60s rtt ├──────────────┤ sort diverge ↓ ├─╮
│                                                                      │
╰─┤ ↑↓ select  ⏎ connect  D divergence  p probe ├──────────────────────╯
```

| PanelOpts field | Reference element |
| --- | --- |
| `key` | the panel number |
| `title` | the panel name |
| `sub` | the dim qualifier after it |
| `right` | the top-right slot |
| `foot_left` | bottom-edge key hints |
| `foot_right` | paging / range |

Do not call `Block::bordered()` for a screen panel. Two panel idioms in one
binary is the thing the port was done to end.

## 4. Colour

`theme::Theme` is the contract: nine themes fill the same named slots, and
`theme::tests::no_screen_paints_from_a_hardcoded_colour` enforces it. Map the
reference's tokens onto slots rather than pasting hex.

| Reference token | Slot |
| --- | --- |
| ground / window / panel | `bg` |
| border-strong | `border` |
| rule | `separator` |
| text | `text_primary` |
| text-mid | `text_secondary` |
| text-dim / dimmer | `text_muted` |
| text-strong | `text_emphasis` |
| cyan (chrome) | `brand`, `key_hint` |
| green (ok) | `status_good`, quiet variant `muted_good` |
| red (failed) | `status_error` |
| blue (memory) | `status_info` |
| magenta (net down) | `rx_rate` |
| grey (never probed) | `absent` |

Six slots the redesign needs and the struct does not have. Add them, and fill
them in all nine palettes:

- `severity_warn` — one divergence (yellow) — and `severity_worst` — the worst
  offender (orange). Today both collapse into `status_warn`, so the hosts table
  cannot distinguish "1 facet" from "the host that breaks the fleet".
- `panel_alt` — title bar and palette card.
- `terminal_bg` — the scrollback pane, darker than the panel.
- `footer_bg` — the footer bar.
- `meter_track` — empty meter cells.

Cyan is chrome: panel numbers, keys, focus, the caret. It never encodes a
value.

## 5. Data the app does not have yet

Three of the six screens need collection that does not exist. The original
listed these beside items already implemented, which understates them by a
large margin.

### 5.1 Per-flow network — new collection path

`monitor::platform` collects one aggregate counter pair per host
(`/proc/net/dev`, `netstat -ib`). There is no socket table and no PID
attribution. The `net · per-flow` panel needs, per sample:

- `ss -tunp` (Linux) / `netstat -anv` (macOS) for the socket table,
- two samples ~1s apart for byte deltas per socket,
- `/proc/net/tcp` inode → `/proc/*/fd` for process attribution — **root for
  sockets owned by other users**.

Rules this imposes on the design, which the reference has no state for:

- Without root, show flows for the session user's own processes and label the
  panel `own processes · run as root for all flows`. Do not silently show a
  partial list as if it were complete.
- If the socket table is unavailable, the panel keeps its aggregate ↓/↑ figures
  and histories, and the flow table shows `per-flow needs ss(8) on the host`.

### 5.2 `peers` median CPU — new sampler

`peers 15%` is the fleet median. Metrics are collected only for the host you
are attached to. This needs a background sampler across probed hosts on the
probe cycle. Until it exists the slot reads `peers —`, not a number.

### 5.3 Forward state — done, and the shape it settled into

Verification turned out to be the wrong frame for local forwards: we own the
listener, so the bind result *is* the state. `portfwd::bind_forward` returns
the `io::Error`, and the row shows `● active`, `● starting` or `● failed` with
the reason. `Active` carries a connection count from a real counter.

Remote (`R`) forwards are refused at the point of request rather than listed.
See §9.

## 6. Degradation

### 6.1 Minimum size

The reference is authored at 150×34. Below that, drop in this order and keep
the rest aligned:

| Screen | Drop first | Then | Floor |
| --- | --- | --- | --- |
| hosts | TAGS, SEEN | RTT sparkline, USER | 60×16 — status, name, address, diverge |
| fleet | facet grid to 2 columns, then 1 | outliers panel | 60×16 — headline + verdict |
| monitor | per-core grid to 1 column | flow table, process RSS | 60×20 — four figures + histories |
| files | transfer panel | remote pane (tab between) | 60×16 — one pane |
| help | grid to 2 columns, then 1 | — | 40×10 — scrolls |

Below the floor, one line: `essh needs 60×16 · this window is 48×12`. Never
draw a partial panel.

`render_smoke::every_view_renders_at_every_size_without_panicking` already
covers the panic case; these are the legibility rules on top of it.

### 6.2 Cells, not pixels

`ch` values are cell counts. Meter widths are given in cells here, so the
reference's `÷5` guess is not needed: agreement meter 20, facet meter 15,
transfer meter 48, per-core meter 8, memory bars 20.

Bar *heights* (9px, 11px), radii, shadows, `line-height` and `letter-spacing`
have no cell equivalent. Drop them. Do not spell `E S S H` to simulate
letter-spacing: it costs columns and breaks header alignment.

Type sizes become weight and colour: 34px → bold + `text_emphasis`; 26px →
bold; 20px → bold; 11.5px hints → `text_muted`.

### 6.3 The `terminal` theme

`Theme::defers_to_terminal()` switches the gradient ramps off so the ANSI-only
theme leaks no 24-bit colour. Gradient cell meters are the redesign's main
quantitative device, so they need a 16-colour form:

- Meters keep `█` fill and `░` track, in one themed colour: `status_good` under
  60%, `status_warn` to 85%, `status_error` above.
- Severity uses the four ANSI slots the theme already pins; yellow and orange
  collapse to one.
- Braille sparklines are unaffected — glyphs, not colour.

The light theme needs severity hues darkened for contrast on a light ground.

## 7. Keys

The redesign adds `p`/`P` and `R` on top of an existing `d`/`D`. Three
shift-pairs, when the defect being fixed was a shiftless-key mixup. Rules:

- A shift-pair is allowed only when the two actions are the same verb at
  different scope (`p` host / `P` fleet). `d` delete and `D` divergence are not,
  and `d` keeps its two-press confirm.
- `r` is unbound for one release and reports the split rather than acting.
- Every binding a footer advertises must be handled on that screen —
  `no_advertised_command_does_nothing` enforces it.

## 8. The seven defects

Six from the original handoff, plus the one it dropped. All seven are fixed;
see `PLAN.md` §0 for the commits and the tests that pin them.

1. `role=web` printed its consensus ratio twice.
2. Forwards reported `Active` with nothing bound.
3. `r` was advertised as probe and performed a reload that discarded probe state.
4. The add-forward format hint was clipped at the panel edge.
5. Help wrapped mid-phrase: `F4 / forwards`, `F9 new / session`.
6. `1 peers never probed`.
7. **A session opened with Enter from the dashboard stayed on "Connecting"
   indefinitely with a blank pane and never collected facets**, while sessions
   opened with F9 connected in about a second. Reported in
   `reference/original-screenshots-README.txt`, absent from the handoff's list.
   It is the only functional defect of the seven, and the hosts screen keeps
   `⏎ connect` as its primary action.

## 9. Out of scope, and what that costs

Unchanged from the original: the launcher, the sessions list, the divergence
modal, the CLI, and the seven theme variants.

Two seams the original does not mention:

- **The divergence modal is the drill-down.** Both hosts and fleet bind `D`,
  and the verdict panel advertises `D facet detail`. The redesign's primary
  drill-down lands on an un-redesigned screen. Either bring it in or accept
  that the flow is half-redesigned; do not discover this during review.
- **Remote port forwards are not implemented.** `tcpip_forward` needs `&mut` on
  the russh handle, which sessions hold behind an `Arc`, and incoming channels
  land in `server_channel_open_forwarded_tcpip`, which drops them. Both are
  fixable; until then `R` specs are refused with a stated reason. The forwards
  panel is therefore local-only, and its title says so.
