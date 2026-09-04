# ESSH TUI redesign v2, corrected handoff

Supersedes `superseded/SPEC-v1.md` and corrects `handoff-v2/README.md`
(2026-09-04). The design direction in the handoff stands. This document fixes
its arithmetic, resolves the three key bindings it double-books, gives every
column a cell budget that fits 150x34, and names the collection work it
presents as layout work.

Read `handoff-v2/redesign-v2.dc.html` for the look. Read this for the numbers,
the widget mapping and the rules.

v2 removes fleet configuration divergence from the product surface. See §8 for
where it goes and what that costs.

## 0. What the handoff gets wrong

Sixteen findings, each with the correction this document adopts. Numbers 1, 3,
5, 12 and 14 are the ones that would have shipped as visible defects.

| # | Handoff | Corrected | Why |
| --- | --- | --- | --- |
| 1 | Header `9 up · 1 busy · 1 offline` over 11 hosts | `7 up · 2 busy · 1 dropped · 1 offline` | The three counts sum to 11 only by filing edge-01 under "up" while the same screen says its connection reset 6 minutes ago. Four states, one host each, §1 |
| 2 | `needs attention │ 3 of 11 hosts` | `4 of 11` | staging-web-01 is offline, and the handoff's own attention rule says offline enters the list. It was missing |
| 3 | Health cards: web-02, db-01, build-01, edge-01 | The four attention hosts | The handoff says the cards are "only the hosts that need watching", then shows a healthy host and omits an offline one |
| 4 | `offline` names both edge-01 and staging-web-01 | `dropped` and `offline` | One had a session and lost it, the other has never answered. They need different words, different glyphs and different recovery keys. This is the v1 "unprobed means two things" defect wearing a new hat |
| 5 | Quick connect lists 5 hosts with no session | 6 | jump-01 has no session and is missing from the panel |
| 6 | Digits bound to tabs, session jumps and quick connect on one screen | Tabs keep 1-4; quick connect takes digits only while focused | Three meanings for `1` on the sessions screen, §4 |
| 7 | Footer is identical on all six screens and advertises `d detach · m monitor` | Footer is per screen | On the files screen `d` is download and `m` is mkdir. `no_advertised_command_does_nothing` fails the build on this |
| 8 | web-02 `12 % cpu` with `load 0.4` on `8 cores` | `load 0.96` | 0.4 across 8 cores is 5% busy. db-01 (61%/4.9) and build-01 (94%/7.8) both use the 8-core relation. Only web-02 breaks it |
| 9 | Sessions row 1 runs `deploy.sh`; the terminal screen shows it finished | deploy.sh still running, prompt not yet returned | Same session, same instant, two answers |
| 10 | Session 2 CPU `61%`, equal to the db-01 host total | `52.4%` | The column is the session's process tree. `postgres: writer` is not in it |
| 11 | Transfer: 120.0 KB file at `52% · 3.1 MB/s · 6s` | 38.8 MB | 48% of 120 KB clears in 20ms, not 6s. At 38.8 MB the rate and the ETA agree |
| 12 | Activity timestamps contradict the durations beside them | Both derived from one clock, §2.3 | `pg_dump started 12:14` next to a session `up 42m` at 12:25 |
| 13 | Palette `5 of 24`, missing `Monitor db-02` | `6 of 28` | 6 sessions + 11 new + 11 monitor is the population; `db` matches 6 of them |
| 14 | Hosts CPU column `8ch`, MEM/DISK `9ch` | 12 / 13 / 13 | The handoff says the `ch` numbers are cell counts. At 8ch the meter gets 3 cells after the percentage, which cannot show a ramp. §3.2 |
| 15 | Session HOST `13ch`, attention host `12ch`, busiest RSS `6ch` | 15 / 15 / 7 | `staging-web-01` is 14 characters and `16.8 MB` is 7 |
| 16 | Add-forward row offers `R HOST:PORT → 127.0.0.1:PORT` | `L` form only | Remote forwards are not implemented and phase 0 refuses `R` with a reason. Advertising them is defect 2 of the original seven, rebuilt |

Five more found while building it, in §0.1.

Two that are not defects in the design but change what the work is:

- **The hosts screen needs metrics the app cannot collect.** §6.1. This is the
  largest single item in v2 and the handoff does not mention it.
- **Six near-black background tokens do not survive the trip to a terminal.**
  §5.2.

## 0.1 Found while building

The review above was done against the design. These came out of implementing
it, which is a different reading and finds different things.

| # | What | Resolution |
| --- | --- | --- |
| 17 | Four states are five. Every host is `unprobed` on the first frame after start, and calling that `offline` is a lie the size of the fleet | `HostState::Unprobed`, a transient of at most one probe cycle. The header prints only its non-zero counts, so at steady state it shows the handoff's four or fewer |
| 18 | The hosts screen's `sort load ↓` would reorder rows every five seconds, under the cursor | Configured order. A live sort moving rows under a cursor is the defect PLAN §0 records twice, and `sort load` belongs on a key, not on the clock |
| 19 | v1's screen 3 listed only outliers, so arriving there snapped the cursor onto a host you had not selected, and `Down` walked rows the hosts screen did not draw | One order for both tabs. v2's health screen lists every host, so the second order is removed rather than reconciled |
| 20 | A single health card stretched across 146 cells puts its label at one edge and its value at the other | Cards keep a fixed 38-cell width whether there is one or four |
| 21 | A collapsed `×N` row truncates its own count off the end of a long command, turning a summed 296% back into what reads as one process | The `×N` is reserved before the name is cut |

And one revision to §8 below, made deliberately rather than found: `D` stays.

## 1. Five host states, and they partition

The handoff uses "offline" for a host whose connection reset and for a host
that has never answered. They arrive from different mechanisms, need different
words and offer different recovery, and conflating them is what produced
findings 1, 2 and 4.

| State | Glyph | Colour | Means | Recovery |
| --- | --- | --- | --- | --- |
| up | `●` | `status_good` | Answers, nothing over threshold | none |
| busy | `●` | `status_warn` | Answers, at least one threshold crossed | none |
| dropped | `●` | `status_error` | Answered, then the connection reset. Scrollback and session survive | `r` reconnect |
| offline | `◌` | `absent` | Has not answered since we started asking | `r` retry |
| unprobed | `◌` | `absent` | Not asked yet | none |

`unprobed` is the fifth, and it is not in the handoff. It lasts at most one
probe cycle, but every host is in it on the first frame after start, and
calling that `offline` would be a lie the size of the fleet. The header prints
only its non-zero counts, so at steady state it shows four or fewer.

The rules that assign them:

1. A host with no successful probe in this run of essh is `offline`. It has no
   last reading, so it shows no figures at all. Never a `0` for unknown.
2. A host that answered and then stopped is `dropped`. It has a last reading,
   which is shown dimmed and labelled as the last reading.
3. A reachable host is `busy` if any of: CPU over 90% sustained for 2 minutes,
   memory over 90%, or any mounted filesystem over 80%. Sustained means every
   sample in the window, so one spike does not flap the count.
4. Otherwise `up`.

Every host lands in exactly one state, so the header's four counts sum to the
host count. This is the property the handoff's three counts do not have.

The health screen's attention list is `busy + dropped + offline`, in that
severity order reversed: dropped, offline, busy. The header and the health
screen therefore cannot disagree, because they read the same partition. Assert
it: `the_header_counts_partition_the_fleet`.

Empty state matters as much as the list. With nothing over threshold the panel
reads `nothing needs attention · 11 hosts reachable, none over threshold`, not
an empty box.

## 2. The canonical fixture

Every number on every screen derives from this table. The design reference, the
demo fleet and the render tests use it, so a number that disagrees with another
screen is a bug rather than a judgement call.

Eleven Debian containers on `127.0.0.11`-`127.0.0.21:2222`, one hostname each,
running a real sshd. `scripts/spec-demo.sh` builds the config from this table.

### 2.1 Hosts

Clock for every screen: `12:25:34`. All hosts have 8 cores.

| host | address | tags | state | sess | cpu | mem | disk / | rtt | load |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| web-01 | 127.0.0.11 | prod web | up | 0 | 8% | 31% | 22% | 0.4 | 0.64 |
| web-02 | 127.0.0.12 | prod web | up | 1 | 12% | 46% | 24% | 0.4 | 0.96 |
| web-03 | 127.0.0.13 | prod web | up | 0 | 15% | 38% | 26% | 0.6 | 1.20 |
| db-01 | 127.0.0.14 | prod db | **busy** disk | 2 | 61% | 72% | **81%** | 0.5 | 4.88 |
| db-02 | 127.0.0.15 | prod db | up | 0 | 14% | 44% | 29% | 0.7 | 1.12 |
| cache-01 | 127.0.0.16 | prod cache | up | 1 | 6% | 58% | 18% | 0.3 | 0.48 |
| edge-01 | 127.0.0.17 | prod edge | **dropped** | 1 | last 9% | last 41% | last 38% | × | — |
| build-01 | 127.0.0.18 | ci build | **busy** cpu | 1 | **94%** | 66% | 47% | 0.5 | 7.52 |
| monitor-01 | 127.0.0.19 | prod monitor | up | 0 | 21% | 52% | 63% | 0.4 | 1.68 |
| staging-web-01 | 127.0.0.20 | staging web | **offline** | 0 | — | — | — | × | — |
| jump-01 | 127.0.0.21 | prod jump | up | 0 | 3% | 19% | 11% | 1.2 | 0.24 |

Header: `● 7 up  ● 2 busy  ● 1 dropped  ◌ 1 offline  │  6 sessions  12:25:34`.
7 + 2 + 1 + 1 = 11. Zero counts are dropped: a header that says `0 dropped`
invites the reader to go looking for what dropped.

`load` is `cpu% × cores`, and the demo asserts the relation rather than the
literal, because the demo fleet's real load is whatever the containers are
doing. See §9.

Sessions per host sum to 6. Hosts with no session, so the six rows of quick
connect: web-01, web-03, db-02, monitor-01, staging-web-01, jump-01.

### 2.2 Sessions

Six sessions on five hosts.

| # | host | state | running | up | cpu | last line | idle |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | web-02 | attached | `deploy.sh` | 14m | 1.4% | `nginx: reloading configuration…` | now |
| 2 | db-01 | detached | `pg_dump` | 11m | 52.4% | `dumping table events … 78%` | 4s |
| 3 | db-01 | detached | `-bash` | 2h | 0.0% | `matt@db-01:~$` | 18m |
| 4 | build-01 | detached | `cargo build` | 11m | 89.3% | `Compiling ratatui v0.29.0` | 1s |
| 5 | cache-01 | detached | `redis-cli` | 1h | 0.2% | `127.0.0.1:6379>` | 32m |
| 6 | edge-01 | dropped | `tail -f` | 3h | — | `connection reset · r reconnect, scrollback kept` | 6m |

CPU is the session's process tree, which is why session 2 reads 52.4% while
db-01 reads 61%: `postgres: writer` belongs to the daemon, not to the session.
Session 4 reads 89.3% against a host at 94% for the same reason.

The dropped session shows `—` for CPU. A dropped session has no live process
tree, and a stale percentage in a live column is the defect §1 exists to stop.

### 2.3 Activity, and the clock

At `12:25:34`, five events inside 15 minutes. Every duration on every other
screen derives from these, so the two can never disagree.

| at | event | what it pins |
| --- | --- | --- |
| 12:21 | `build-01 cpu over 90%` | attention row `over 90% for 4m` |
| 12:19 | `edge-01 session dropped` | hosts row `connection reset 6m ago`, session 6 idle `6m` |
| 12:14 | `db-01 pg_dump started` | session 2 `up 11m` |
| 12:11 | `web-02 attached` | session 1 `up 14m`, terminal meta `up 14m` |
| 12:10 | `staging-web-01 went offline` | hosts row `offline since 12:10` |

### 2.4 Health

Attention, four rows, dropped then offline then busy:

```
edge-01         dropped   connection reset 6m ago, 1 session waiting · scrollback preserved
staging-web-01  offline   no answer on 127.0.0.20:2222 since 12:10 · no session was open
build-01        cpu 94%   over 90% for 4m — cargo build in session 4 · expected during a build
db-01           disk 81%  / has 71 GB free, filling ~4 GB/day · pg_dump writing to /backups
```

Each row names the cause when one is known, and says when the condition is
expected. A busy host with a named cause is not an incident.

Cards, one per attention row, in the same order. edge-01 and staging-web-01 are
both stale cards and they are not the same stale:

- edge-01 has a last reading. Dim card, `—` as the figure, the last values
  behind it, dead sparkline, and `Connection reset 6m ago. Values are the last
  reading, not live.` Hints `r reconnect · s silence 1h`.
- staging-web-01 has no reading at all. Same dim card, `—` throughout, no
  values behind it, and `Offline since 12:10. Never reached, nothing to show.`
  Hints `r retry · s silence 1h`.

Busiest right now, across all hosts:

| host | process | cpu | rss |
| --- | --- | --- | --- |
| build-01 | rustc | 88.2 | 2.1 GB |
| db-01 | postgres: pg_dump | 52.4 | 840 MB |
| db-01 | postgres: writer | 7.1 | 210 MB |
| monitor-01 | prometheus | 5.2 | 1.3 GB |
| web-02 | nginx: master | 3.1 | 1.7 MB |

build-01's 88.2 sits under a host reading 94%, and db-01's 52.4 + 7.1 under 61%.
Both leave room for the rest of the process table, which is the correct
relation. A top-N list that sums to the headline is a lie about the tail.

### 2.5 Terminal, on web-02

Scrollback ends mid-command, because session 1 is `deploy.sh` and the sessions
table says so:

```
matt@web-02:~$ uptime; free -h | head -2
 12:11:02 up 12 days, 22:02,  1 user,  load average: 0.96, 1.02, 0.88
               total   used   free  shared  buff/cache  available
Mem:            62Gi   29Gi   14Gi    6.5Gi       20Gi        32Gi
matt@web-02:~$ ./deploy.sh --release 2.14.4
→ unpacking app-2.14.4.tar.gz
→ running migrations … none pending
→ reloading nginx
nginx: reloading configuration…▊
```

Meta line: `web-02 · matt@127.0.0.12:2222 │ up 14m │ rtt 0.4ms`, right-aligned
`1 of 2 forwards active` in `status_warn`. The handoff's `2 forwards` counts
rules; the forwards panel says one of them failed to bind. Count what bound.

Strip: cpu `12 %` `load 0.96 · 8 cores`; memory `30.6 GB / 66.7 · 46%`; net
`↓ 8.4 M ↑ 1.2 M`; top by cpu with `nginx: worker ×6` collapsed.

`free -h` reports GiB and the meter reports GB off a later sample. 62 GiB is
66.7 GB and 29 GiB is 31.1 GB, so the two agree to their rounding and the spec
does not require them to agree to the digit. It does require the meter's
percentage to come from the same sample as its figures.

### 2.6 Files and forwards

Local `…/essh-demo/home/Downloads`, remote `web-02:/home/matt`.

`app-2.14.4.tar.gz` is 38.8 MB locally and 20.2 MB remotely, because it is 52%
uploaded. That row is marked in transfer, not as a collision: a partial upload
of the file you are uploading is not a conflict. The collision demo is
`deploy.sh`, 1.2 KB local and 46 B remote, yellow on both sides.

Transfer: `52% · 3.1 MB/s · 6s`. 48% of 38.8 MB is 18.6 MB, and 18.6 / 3.1 is
6.0. Always show rate and ETA, never a bare bar.

Forwards on web-02, two rules, one bound:

```
L →  127.0.0.1:8080    localhost:80    ● failed  no listener on 8080 — bind refused, 2s ago · r retry
L →  127.0.0.1:15432   db-01:5432      ● active  probed 2s ago · 1 open connection
```

State is the bind result, not an assumption. See §6.3.

### 2.7 Command palette

Population 28: attach one of 6 sessions, open a new session on one of 11 hosts,
monitor one of 11 hosts. Query `db` matches 6.

```
❯ db                                                              6 of 28
→ Attach session 2      db-01   pg_dump
→ Attach session 3      db-01   -bash
+ New session           db-01
+ New session           db-02
◔ Monitor               db-01   disk 81%
◔ Monitor               db-02
```

Results are sessions and hosts, never settings.

## 3. Cell budgets

The handoff says the `ch` numbers are cell counts. They are, and at 150 columns
several of them do not fit their content. Every grid below is arithmetic
against a 150x34 terminal, which is the geometry the reference was authored at.

Rows: 1 header, 1 footer, 32 for the body. Panel gaps are 1 blank row and 2
blank columns, per the handoff's translation table.

Inner width of a full-width panel is `150 - 2 border - 2 padding = 146`.
A `14px` column is 1 cell. A `14px` gap is 2 cells.

### 3.1 Sessions

`3 1 15 9 11 8 7 Fill 9`, gaps 2, eight of them.

```
fixed 3+1+15+9+11+8+7+9 = 63   gaps 16   LAST LINE = 146 - 79 = 67
```

HOST is 15, not the handoff's 13: `staging-web-01` is 14 characters and the
hosts table already budgets 15 for the same value. RUNNING at 11 fits
`cargo build` exactly and ellipsises anything longer. LAST LINE at 67 is the
column the screen exists for, so it takes the Fill.

Selected row: `row_selected` background plus `▌` in the gutter, which is the
dot column. No column shifts, because the marker replaces the dot rather than
preceding it. The dot's colour moves onto the hostname for that row.

The `#` column is an identifier, not a binding. It is the number
`Ctrl+A <n>` jumps to inside a session. The panel subtitle says so:
`6 open on 5 hosts · Ctrl+A 1-9 jumps`.

Bottom row: three panels at `Fill Fill Fill`, gap 2, so 48 / 48 / 48, inner 44
each. Quick connect's six rows fit in 8 lines with the border and the footnote.

### 3.2 Hosts

`1 15 14 6 12 13 13 6 12 Fill`, gaps 2, nine of them.

```
fixed 1+15+14+6+12+13+13+6+12 = 92   gaps 18   TAGS = 146 - 110 = 36
```

CPU 12, MEM 13, DISK 13 against the handoff's 8, 9, 9. Each is a `Fill 4`
sub-grid with a 1-cell gap, so the meter gets 7, 8 and 8 cells. Three cells,
which is what `8ch` yields, cannot express a ramp: it is on, half, or full.
Eight cells is the btop idiom and the smallest width where the gradient reads.

TAGS at 36 is generous for `prod web`. It absorbs the widening and stays Fill.

**Order is the configured order.** The handoff's top-right slot reads
`sort load ↓`, and a load sort on a five-second poll reorders the table while
you are looking at it: the row under the cursor becomes a different machine
between one frame and the next, and the next `Enter` connects to it. That is
the defect PLAN §0 records twice, once for a divergence sort and once for a
reload. Sorting belongs on a key, not on the clock.

Unreachable rows replace the five measurement columns with one explanation
spanning CPU through RTT 60s:

```
12 + 13 + 13 + 6 + 12 + 4 gaps × 2 = 64 cells
```

The handoff's edge-01 string is 65 characters, one over. Do not hand-trim it.
Build the reason from parts and drop them in a fixed order until it fits:

1. `connection reset 6m ago` or `offline since 12:10`, never dropped
2. ` · r reconnect` or ` · r retry`, never dropped
3. ` · last seen cpu 9%, mem 41%`, dropped first
4. ` · no response on 2222`, dropped second

At 64 cells edge-01 keeps parts 1, 2 and 3 at 64 exactly after `last seen` is
shortened to `last`. Below 64 it degrades to parts 1 and 2, which is the floor
and always fits. Assert the floor, not the string:
`an_unreachable_row_always_states_its_recovery_key`.

### 3.3 Health

Attention: `15 9 Fill`, gaps 2. `146 - 15 - 9 - 4 = 118` for the sentence.
Host 15, not 12, for `staging-web-01`. Verdict 9 fits `disk 81%`.

Cards: fixed at 38 cells each, gaps 2, inner 34. Fixed rather than `Fill`,
because a single card stretched across 146 cells puts its label at one edge of
the screen and its value at the other, with a hundred cells of nothing between
them. A stale card keeps the live geometry so its columns do not move when a
host goes away. Longest internals are
`● staging-web-01 │ offline` at 26 and `up 12d · deploy.sh running` at 26.
Below 4 cards' worth of width, drop to 2 columns and then 1, oldest attention
last.

Bottom row: two panels, `(146 - 2) / 2 = 72`, inner 68.

- busiest `14 Fill 5 7`, gaps 2, three of them: PROCESS = 68 - 26 - 6 = 36.
  RSS is 7, not 6: `16.8 MB` is 7 characters.
- reachability `15 Fill 6`, gaps 4: the sparkline gets 43 cells. The handoff
  says 20, which is the reference's pixel width, not a rule. 43 cells of a
  10-minute buffer is one cell per 14 seconds and it is free, so take it.

### 3.4 Files and forwards

Forwards `4 22 20 12 Fill`, gaps 2, four of them: LAST CHECK = 146 - 58 - 8 =
80. The failed row's reason is 51.

The add-forward row sits outside the table, on rows of its own, and carries
both directions now that both work:

```
a  add forward   L PORT:HOST:PORT   here → there: we listen, they answer
                 R PORT:HOST:PORT   there → here: they listen, we answer
```

LAST CHECK says when the state was established, not when it was probed:
`bound 4s ago`, `no listener on 8080 — bind refused · just now`. §6.3 settled
on the bind result *being* the state, so a probe time would be a claim about a
check nobody ran. The stamp moves only when the state changes; a connection
count ticking up is not a rebind.

### 3.5 Help

`2fr 1fr`, gap 2: keys 96, config 48.

Keys inner 92, two labelled columns at 45 with a 2 gap, each `11 Fill`, so the
description gets 32. The longest is `quit (sessions keep running)` at 28, and
`terminal + monitor split` at 24. The handoff's `1.25fr 1fr` gives 24, which
clips the first of those. That is defect 4 of the original seven rebuilt at a
different width, which is why the budget is written down here.

`Ctrl+A ←/→` is 11 characters and sets the key column.

Config inner 44, `8 Fill`, so 34. Entries are two lines when the value is
longer: `config.toml` then a dim `11 hosts · edited 4h ago`.

Function keys: six, in three columns of 30 inside the keys panel.

### 3.6 Floors

Below these, one line: `essh needs 60x16 · this window is 48x12`. Never a
partial panel.

| Screen | Drop first | Then | Floor |
| --- | --- | --- | --- |
| sessions | bottom row of three panels | UP, CPU, IDLE | 60x16, `#` host state last line |
| hosts | TAGS, RTT 60s | MEM, DISK meters to bare percentages | 60x16, dot name address cpu |
| health | reachability panel, then busiest | cards to 1 column | 60x16, attention list alone |
| terminal | strip | meta line to host only | 60x16, scrollback |
| files | transfer panel | remote pane, tab between | 60x16, one pane |
| help | keys grid to 1 column | config panel | 40x10, scrolls |

`render_smoke::every_view_renders_at_every_size_without_panicking` already
covers the panic case. These are the legibility rules on top of it.

## 4. Keys

The handoff double-books digits three ways and the footer twice. Both need
resolving before any of it is built, because a key map is not something you
discover during review.

### 4.1 Digits

On the sessions screen the handoff wants `1` to switch to the sessions tab
(header), jump to session 1 (the `#` column), and open a session on web-01
(quick connect). Resolution:

- `1`-`4` switch dashboard tabs, everywhere on the dashboard. This is what the
  header numbers mean, what the help screen teaches, and what already ships.
- The sessions `#` column is an identifier for `Ctrl+A <n>`, which only exists
  inside a session. §3.1 says so in the panel subtitle. It is not a dashboard
  binding.
- Quick connect keeps its numbers, but they only fire while the panel holds
  focus. `Tab` and `Shift+Tab` cycle panel focus within a screen, the focused
  panel takes `border_focus`, and the footnote reads
  `Tab here, then press the number.`

The focus ring is already in the design language, the design already tokens it,
and it is the only resolution that keeps all three affordances. The cost is one
extra keystroke on quick connect, which is the least valuable of the three.

### 4.2 The footer

The handoff's footer is identical on all six screens and advertises
`d detach · m monitor`. On the files screen `d` is download and `m` is mkdir.

The footer is per screen. It carries the current screen's bindings, then a
stable tail that is bound everywhere: `F1 help · ^P menu · q quit`, and
right-aligned `theme dark · 150×34`. `no_advertised_command_does_nothing`
already fails the build when a footer advertises a key the screen ignores, so
this is enforced rather than remembered.

### 4.3 Shift pairs

A shift pair is allowed only when the two actions are the same verb at
different scope. v2 keeps `Ctrl+A M` split and `Ctrl+A m` full screen, which
qualifies: same verb, two layouts. `d` delete and `D` divergence did not, and
divergence leaves the dashboard anyway (§8).

`r` is uniformly reconnect or retry: on a dropped session, an offline host row,
a stale card, and a failed forward. It is never probe and never reload.

### 4.4 The map

```
DASHBOARD                          IN A SESSION
1-4     switch tab                 Ctrl+A 1-9  jump to session N
j k ↑ ↓ navigate list              Ctrl+A ←/→  previous / next
Tab     next panel                 Ctrl+A Tab  last session
Enter   connect / attach           Ctrl+A M    terminal + monitor split
n       new session here           Ctrl+A m    full-screen monitor
d       detach session             Ctrl+A f    file browser
r       reconnect / retry host     Ctrl+A p    port forwards
m       monitor selected host      Ctrl+A b    broadcast to a tag
a       add host                   Ctrl+A d    detach, keep it running
e       edit host                  Ctrl+A w    close session
/       search hosts
Ctrl+P  command menu
q       quit (sessions keep running)

F1 help   F2 monitor   F3 files   F4 forwards   F5 mini strip   F9 new session
```

`q quit (sessions keep running)` states the persistence guarantee where the
anxiety is. It appears in three places: this map, the sessions panel subtitle,
and the config panel.

## 5. Colour

### 5.1 Slots

`theme::Theme` is the contract. Nine themes fill the same named slots and
`theme::tests::no_screen_paints_from_a_hardcoded_colour` fails the build when a
screen bypasses them. Map the reference's tokens onto slots rather than pasting
hex.

| Reference token | Slot |
| --- | --- |
| ground, window, panel | `bg` |
| border-strong | `border` |
| border-focus | `border_focus` *(new)* |
| rule | `separator` |
| text | `text_primary` |
| text-mid | `text_secondary` |
| text-dim, text-dimmer | `text_muted` |
| text-strong | `text_emphasis` |
| cyan (chrome) | `brand`, `key_hint` |
| green (ok) | `status_good`, quiet variant `muted_good` |
| yellow (busy) | `status_warn` |
| red (bad) | `status_error` |
| blue (memory) | `status_info` |
| magenta (net down) | `rx_rate` |
| grey (offline) | `absent` |
| panel-alt | `panel_alt` *(new)* |
| terminal | `terminal_bg` *(new)* |
| footer | `footer_bg` *(new)* |
| meter-track | `meter_track` *(new)* |

Five new slots, filled in all nine palettes. v1's spec asked for six because it
split `status_warn` into warn and worst for the divergence ramp. v2 has no
divergence, so `status_warn` stays whole and `border_focus` takes the sixth
place: §4.1 makes panel focus load-bearing, and it cannot be drawn from
`border`.

Cyan is chrome. Panel numbers, keys, focus, the caret. It never encodes a
measurement.

### 5.2 Six backgrounds do not survive

The handoff's tokens include ground `#07090b`, window `#0a0d10`, panel
`#0c1014`, panel-alt `#0e1216`, terminal `#080b0e` and footer `#0b0f13`. Those
are six values inside a 7-point luminance range. On a terminal they are
indistinguishable at normal brightness, and under `terminal` they cannot be
expressed at all, because that theme pins no RGB by design.

Collapse to three: `bg` for ground, window and panel; `panel_alt` for the title
strip and the palette card; `terminal_bg` for scrollback and command input.
`footer_bg` is `panel_alt`. The separation the reference gets from luminance
comes from the border and the rule in a TUI, which is how btop does it too.

### 5.3 Meters

`design::meter_ramped(fraction, width, ramp)` draws a run of `█` coloured by
**position along the scale**, not by value, with `meter_track` behind the empty
cells. That is the btop device the reference is imitating, and colouring by
value instead makes a 40% meter and a 95% meter the same colour at their left
edge.

CPU uses the utilisation ramp, green to yellow at 60% to red. MEM is flat
`status_info`. DISK is flat `muted_good` switching to `status_warn` past 80%.
The number beside the meter takes the severity colour; the meter does not
change colour to match.

Under `Theme::defers_to_terminal()` the ramps switch off, because a five-stop
gradient has no 16-colour form. Meters keep `█` fill and `░` track in one
themed colour: `status_good` under 60%, `status_warn` to 85%, `status_error`
above. Braille sparklines are unaffected, being glyphs rather than colour.

The light theme needs the four severity hues darkened for contrast on a light
ground.

## 6. Data the app does not have

Three of the six screens need collection that does not exist. The handoff
presents all of it as layout, which understates v2 by a wide margin. Nothing
that renders a number ships before the data behind it exists.

### 6.1 Fleet metrics, the long pole

The hosts screen's CPU, MEM, DISK and the health screen's cards, attention
rules and busiest-process list all need per-host metrics for **every** host,
including the ten with no session open.

What exists today:

- `fleet::FleetProber` does a TCP connect every 60s. It yields reachable, an
  RTT and the sparkline history. Nothing else.
- `monitor::HostMetricsCollector::collect` needs an established
  `russh::client::Handle`, which only exists for a host you have a session on.
- `divergence::collect` has the same constraint, which is why the v0.3.3
  screenshots show facets for nine hosts: nine sessions were open.

So v2's primary screen is built on a sampler that does not exist. It needs:

1. A pool of SSH connections to configured hosts, opened lazily and kept warm,
   or a connect-run-close per poll. Warm connections cost 11 idle channels and
   file descriptors; connect-per-poll costs a full handshake every 5 seconds
   per host. Measure both against the fixture fleet before choosing. A
   handshake to a container on loopback is not a handshake to a host three
   network hops away, so measure with added latency.
2. One command per sample, not five. `monitor::platform` already builds a
   combined command with section sentinels for the attached host. Reuse it.
3. A concurrency limit and a jitter. Eleven hosts is nothing; a real fleet is
   200, and 200 simultaneous handshakes every 5 seconds is a denial of service
   you wrote yourself.
4. Failure that degrades to the state model in §1 rather than to zeroes. A host
   that answers TCP but refuses the exec is reachable with no metrics, which is
   a fourth thing the table has to say.
5. A back-off. A host that has failed its last three samples moves to the 60s
   probe cycle until it answers.

The poll interval is 5s per the handoff. That is a reasonable default for a
dashboard and an unreasonable one for 200 hosts, so it is configurable and the
config panel already shows it.

Until this lands, the hosts screen shows the columns it can fill and says why
the others are blank. It does not show zeroes.

### 6.2 Session detail

The sessions table's RUNNING, CPU and LAST LINE columns are also new.

- **LAST LINE** is the last non-empty line of the session's scrollback. Sessions
  already keep a `vt100` parser, so this is a read, not a collection. It is the
  column that makes the screen worth having, and it is the cheapest of the
  three.
- **RUNNING** is the foreground process of the session's TTY. One `ps` per
  session per poll, or read it from the pty's foreground process group where
  the platform allows.
- **CPU** is the session's process tree, summed. Same `ps`, one more column.

A dropped session has no live values. It keeps its last line, because the
scrollback survived, and shows `—` for the rest.

### 6.3 Forward state, settled

Verification turned out to be the wrong frame for local forwards. We own the
listener, so the bind result is the state. `portfwd::bind_forward` returns the
`io::Error`, and the row shows `● active`, `● starting` or `● failed` with the
reason. Active carries a connection count from a real counter. Shipped in
phase 0.

### 6.4 Remote forwards, done, and what the two blockers turned out to be

Only one of them was real.

**`tcpip_forward` needs `&mut`.** It does, and it does not need to:
`cancel_tcpip_forward` sits next to it in russh 0.55 taking `&self`, and both
bodies send one message on an mpsc and await a oneshot. The `&mut` is
incidental. We cannot change that from here, so the choice was what to pay
instead.

`Arc<Mutex<Handle>>` was this document's first answer and it is the wrong one.
Every existing caller shares that handle — the metrics sweep, the divergence
collector, the file browser, the shell — so the lock is contended by
everything. Held across a sweep it blocks the shell; held narrowly it means
changing eight helper signatures that take `&Handle`.

What ships instead: a second connection, opened the first time a session asks
for a remote forward and used for nothing else. It costs one handshake and one
extra line in the host's auth log, paid only by sessions that use the feature.
That is a different trade from the sampler's in §6.1, where the alternative was
a handshake per poll forever, and it leaves 31 call sites alone.

**Channels were dropped.** This one was real and worse than it sounds.
`server_channel_open_forwarded_tcpip` accepted the channel and returned, so
whoever connected on the remote side got a socket that would never answer. A
hang, not a refusal: the far end cannot tell it from a slow service.

`portfwd::remote::Router` holds the routes and the handler proxies to them. It
matches on the port alone when the address does not match, because OpenSSH
reports the address it bound rather than the string it was asked for, and
refusing over that mismatch would be pedantry with a hang attached.

Deleting a remote forward tells the server. A row removed while the server
keeps listening is the same class of lie as a row listed while nothing is
bound, one layer further away.

`fleet::remote_forward_live` proves it against a real sshd: the server accepts,
the channel comes back, the bytes arrive, the connection is counted, and
cancelling stops the server listening.

### 6.5 Broadcast

`essh run <tag> -- <cmd>` already fans out over one-shot connections and prints
per-host output. The broadcast panel needs the same fan-out reporting exit code
and duration per host into the UI instead of stdout, without blocking the
frame. The CLI path is the model; the streaming is new.

### 6.6 Activity

The activity panel needs an event log, which does not exist. Entries come from
state transitions the app already computes: a session's state changing, a host
crossing or clearing a threshold, a host changing reachability. Ring buffer,
capped at the panel's height times four, timestamps from one clock so §2.3
holds.

## 7. Panels

Use `design::panel::pane` with `PanelOpts`. It is netwatch's idiom, already
ported, and it carries everything the reference draws on the border.

```
╭─┤1├─┤ hosts ├─┤ 11 hosts · refreshed 4s ago ├────────────┤ sort load ↓ ├─╮
│                                                                          │
╰─┤ ↑↓ select  ⏎ connect  m monitor  a add  e edit  / search ├─────────────╯
```

| `PanelOpts` field | Reference element |
| --- | --- |
| `key` | the panel number |
| `title` | the panel name |
| `sub` | the dim qualifier after it |
| `right` | the top-right slot |
| `foot_left` | bottom-edge key hints |
| `foot_right` | paging or range |

Do not call `Block::bordered()` for a screen panel. Two panel idioms in one
binary is the thing the port was done to end.

Panel numbers are decoration on every screen. They are not bindings, because
§4.1 gives digits to the tabs. They stay because they are the btop idiom the
design is built on and because they give the key hints something to refer to.

## 8. What v2 drops, and what that costs

Gone from the product surface: fleet consensus scoring, the `DIVERGE` column,
tag and group agreement bars, per-facet percentages, host verdicts with config
hashes, and the `p` / `P` / `R` probe-versus-reload split.

The engine is not gone. `divergence::` collects facts, scores categorical
agreement, refuses to name a consensus on a tie, and has 100-odd tests. Deleting
it to satisfy a screen redesign would be the expensive kind of tidy. It moves
behind explicit commands:

- `essh diff <tag>` compares a peer set and prints the facet table.
- `essh why <host>` explains one host's divergence. Already shipped.

Both require sessions or one-shot connections to the peer set, which is the
same fan-out §6.1 needs. Build it once.

**`D` stays, and this is a revision to the first draft of this section.**

The draft said v2 has no `D` and no modal, so a user who wants to know why two
hosts differ leaves the TUI. Implementing it made that look wrong. The
handoff's argument is about the *default dashboard*: consensus percentages sat
in a column every user saw and had to interpret. A key you have to know about
is not on the default dashboard and taxes nobody, and removing it would strand
a working drill-down behind a CLI command the TUI never mentions.

So: the `DIVERGE` column, the groups panel and the consensus screen are gone,
`D` on the hosts screen still opens the divergence view for the host under the
cursor, and `essh diff` exists for the same question outside the TUI. What v2
drops is divergence as something you are shown; what it keeps is divergence as
something you can ask for.

Still out of scope, unchanged: the launcher, add-host and edit-host forms, the
search overlay, and the full-screen monitor for one host.

## 9. Verification

The demo and `tests/demo_verify.rs` drive the same binary through the same keys
against the same fleet. The GIF is what a person looks at. The test is what
verifies it, because a screen is text before it is pixels and "a count agrees
with its noun" is an assertion rather than an impression.

`scripts/spec-demo.sh` refuses to publish a GIF whose beats do not hold.

Rules asserted on every frame, not only the frame that shows them:

1. No count disagrees with its noun.
2. No binding is split across rows by wrapping.
3. The header's four state counts sum to the host count (§1).
4. No footer advertises a key the screen ignores.
5. `r` is never advertised as probe or reload.
6. An unreachable row states its recovery key, whatever the width.
7. No stale figure appears in a live column. A dropped host's last reading is
   labelled or it is `—`.

Per beat, the specific claims of §2. Fixture literals are asserted as
relations, not values, because the demo fleet is eleven real containers and
their CPU is whatever they are doing:

- `load ≈ cpu% × cores`, within the sampling window.
- A session's CPU is at most its host's CPU.
- Sessions per host sum to the header's session count.
- Quick connect lists exactly the hosts with no session.
- The attention list equals `busy + dropped + offline` from the header.
- The transfer's ETA equals remaining bytes over the reported rate.

`scripts/fleet.sh` builds and stages the fixture: eleven Debian containers on
`127.0.0.11`-`.21:2222`, one hostname each, running a real sshd.

Two things about it are not decoration.

**staging-web-01 is stopped, not staged.** The offline state in the demo is a
container that is actually down, so the row essh draws for it comes from a
probe that actually failed. A fixture where every host answers cannot verify a
screen whose first panel is about hosts that do not.

**build-01 runs real load.** Three nice'd busy loops, named
`essh-fixture-load` so `ps` reports something a reader can act on. The
attention row has to name a process the sampler actually found; staging that
by drawing it would verify nothing, and it is what makes the `×N` collapse
testable.

Staging is part of the fixture and is scripted like the rest of it, because it
lives in the containers' running state rather than their filesystem. A restart
keeps nginx installed and loses nginx listening, so the fleet comes back
reachable and unstaged, and `demo_verify` then fails on "web-02 is not serving
:80" and blames the app for a fact about the laptop. `./scripts/fleet.sh check`
reports that drift without changing anything.

### 9.1 What the container fleet cannot show

Containers share the host kernel and, without lxcfs, the host's `/proc`. All
eleven therefore report the *laptop's* CPU, memory and disk, identically. The
plumbing is real and so is every reading, but the numbers do not differ per
host, so the fixture cannot demonstrate one host busy and another idle on the
hosts table.

What it does verify: that the sampler reaches hosts with no session, that the
columns fill from live readings, that unknown is `—` and never `0`, that an
unreachable host states its recovery key, that the header's counts partition,
and that the attention list agrees with them. `ps` *is* per-container, so the
busiest panel, the sibling collapse and the cause attribution are all real.

Making per-host CPU differ needs either lxcfs in the fixture image or a
sampler that reads cgroup counters when `/proc` is the host's. Neither is
needed to verify the screens; both are needed before a screenshot of this
fixture means anything about a real fleet.
