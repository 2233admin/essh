# ESSH TUI redesign v2, implementation plan

Companion to `SPEC.md`. Phases 0 to 3 are committed on `redesign/v2`; phases 4
to 9 are sized against the code as it stands after them.

| Phase | State |
| --- | --- |
| 0 · the seven v0.3.3 defects | done, `5d141f0` |
| 1 · design system | done, `a5005aa` |
| 2 · fleet metrics | done, `a3a0ae0` |
| 3 · hosts and health | done, `a3a0ae0` and `261ef86` |
| 4 · sessions | done, `08e750c` |
| 5 · terminal, files, forwards | done, `54efaff` |
| 6 · help and config | done, `8817693` |
| 7 · themes | done, `682cad6` |
| 8 · divergence moves out | done, `99a9e90`; `essh diff` ships and `D` stays (SPEC §8) |
| 9 · remote port forwards | done |

Ordering rule: nothing that renders a number ships before the data behind it
exists. The v1 plan opened with that warning about three screens. v2 makes it
worse, not better. The hosts screen is now the health screen's data source and
it needs metrics for every configured host, which the app has never collected
for a host without a session (SPEC §6.1). Most of v2 is collection work wearing
a layout costume.

## Phase 0, the seven v0.3.3 defects (done)

Committed. `cargo test`: 461 unit + 21 integration pass, 9 benches ignored.
`cargo clippy --all-targets`: clean.

| # | Fix | Files | Pinned by |
| --- | --- | --- | --- |
| 1 | `note()` no longer repeats the ratio the column prints | `divergence/mod.rs` | `a_consensus_ratio_is_printed_exactly_once` |
| 2 | `bind_forward` returns the `io::Error`; `ForwardState` replaces `active: bool`; forwards are started and their listeners released on delete | `portfwd/mod.rs`, `main.rs`, `tui/portfwd_view.rs`, `tui/mod.rs` | `a_forward_that_did_not_bind_is_never_shown_as_active`, `a_bind_that_fails_says_why`, `the_summary_counts_only_forwards_that_bound` |
| 3 | `p` probe host, `P` probe fleet, `R` reload; `apply_probe_state` puts measurements back after a reload | `main.rs`, `tui/dashboard.rs` | `no_advertised_command_does_nothing` |
| 4 | Add form gets its own rows, panel widened, reasons kept short | `tui/portfwd_view.rs`, `main.rs` | `the_add_form_format_is_never_clipped` |
| 5 | `key_grid` lays bindings out as indivisible cells | `tui/help.rs` | `no_binding_is_split_across_rows` (5 widths) |
| 6 | `format::count` agrees the noun with the number | `format.rs`, `divergence/`, `tui/` | `counts_agree_with_their_noun` |
| 7 | `discard_unconnected_sessions`; no path leaves a session on "Connecting" | `main.rs` | `a_session_that_never_connected_does_not_survive_the_attempt` |

Phase 0 predates the v2 handoff and targets v1's screens. Fixes 2, 4, 5, 6 and
7 carry over unchanged. Fix 1 and fix 3 are about screens v2 deletes; the code
survives behind `essh why` and `essh diff` (SPEC §8), and the `p` / `P` / `R`
split retires with the dashboard tab that used it.

## Phase 1, the design system (done)

Nothing renders differently at the end of this phase. It is the foundation the
other screens assume, and it is the phase where a redesign quietly regresses,
so it gates on a slot table rendered in all nine themes.

1. Five slots on `theme::Theme`: `panel_alt`, `terminal_bg`, `footer_bg`,
   `meter_track`, `border_focus`. Fill them in all nine palettes.
   `no_screen_paints_from_a_hardcoded_colour` already fails the build if a
   screen bypasses them. SPEC §5.1.
2. `design::meter_ramped(fraction, width, ramp)`, coloured by position along
   the scale rather than by value, with `meter_track` behind. Falls back to
   `design::meter`'s single colour when `Theme::defers_to_terminal()`. Test
   both branches. SPEC §5.3.
3. Shared global header and footer widgets. The active tab is **derived** from
   `AppView` plus `DashboardTab`, never passed in, so terminal and files map to
   `1 sessions` without a call site remembering to. SPEC §4.2 makes the footer
   per screen.
4. `HostState` as a real type with the four variants of SPEC §1, and
   `fleet_counts()` returning a partition. `HostStatus` today has three
   variants and no notion of busy.

**Done when** a table of every slot renders in all nine themes, the terminal
theme emits no RGB, and `the_header_counts_partition_the_fleet` passes.

## Phase 2, fleet metrics (done)

Collection first. Every panel in phases 3 and 4 reads this.

1. `fleet::MetricsSampler`: a connection strategy over configured hosts on the
   poll cycle, with a concurrency limit, jitter, and back-off to the 60s probe
   cycle after three failures. SPEC §6.1.
2. Decide warm pool versus connect-per-poll by measurement, against the fixture
   fleet with added latency. Write the number down in the module doc; a future
   reader will otherwise re-litigate it.
3. One combined command per sample with section sentinels. `monitor::platform`
   already builds this for the attached host. Reuse it rather than growing a
   second command builder.
4. Reachable-but-no-metrics is a state the table has to say, not a zero.
5. `HostDisplay` grows `cpu`, `mem`, `disk`, `load`, `cores`, `last_sample_at`,
   and each is an `Option`. `None` renders `—`.

**Done when** the fixture fleet's eleven hosts all report on the 5s cycle with
no session open, a stopped container degrades to `offline` inside two cycles
without a zero appearing anywhere, and the sampler's cost is measured and
recorded.

Done. The measurement and the choice it drove are in `fleet::sampler`'s module
doc. `fleet::live` runs the sampler against the containers, because the unit
tests parse canned output and cannot tell you whether the command essh sends
produces output essh can read — which is exactly the failure that happened,
with eight open connections and a table of dashes.

## Phase 3, hosts and health (done)

The two screens the redesign is for. `DashboardTab::Fleet` becomes `Health`.

1. Hosts table to SPEC §3.2's columns and cell budget. Ramped CPU meter, flat
   MEM and DISK, RTT sparkline, tags without `env=` prefixes.
2. Unreachable rows: the spanning explanation with the degradation order of
   SPEC §3.2, asserted at the floor rather than as a string.
3. Attention list from the §1 partition, with cause attribution. A busy host
   with a named cause is not an incident, and the sentence has to say so.
4. Health cards, four across, with the two stale variants of SPEC §2.4 kept
   distinct: a last reading, versus never reached.
5. Busiest-across-the-fleet list. Needs the sampler's process table, so it
   gates on phase 2 step 3.
6. Reachability panel, 10-minute buffer, sparkline at the Fill width.

**Done when** a fixture-driven render test asserts SPEC §2's relations on both
screens and no count on either disagrees with the header.

Done, in `tests/demo_verify.rs` against the live fixture. What it cannot
assert is in SPEC §9.1: the containers share the host's `/proc`, so per-host
CPU cannot differ. Fixing that means lxcfs in the fixture image or a sampler
that reads cgroup counters — needed before a screenshot of this fixture says
anything about a real fleet, not needed to verify the screens.

## Phase 4, sessions (3 to 4 days)

The landing screen, and the one v2 promotes to primary.

1. LAST LINE from the existing `vt100` scrollback. Cheapest of the three new
   columns and the reason the screen exists. Do this first and alone: it is
   shippable on its own.
2. RUNNING and CPU per session, one `ps` per session per poll. SPEC §6.2.
3. Quick connect, with the focus model of SPEC §4.1. `Tab` cycles panels,
   `border_focus` shows which, digits fire only while focused.
4. Activity ring buffer from state transitions the app already computes.
   SPEC §6.6.
5. Broadcast panel over the `essh run` fan-out, streaming exit code and
   duration without blocking the frame. SPEC §6.5.

Steps 4 and 5 are independent of 1 to 3 and of each other.

## Phase 5, terminal, files, forwards (3 days)

1. Session chips with per-host severity dots, so trouble is visible without
   leaving the session. Meta line counts forwards that **bound**, not rules.
2. The four-panel strip at the same panel chrome.
3. Files: pre-shortened LTR paths. Not the `direction` trick, which reorders
   the neutral `/` and displays a path that is not the real one. Collision
   highlighting, transfer rate and ETA.
4. Forwards to the phase 0 state model, add-forward row `L` only.

## Phase 6, help and config (1 to 2 days)

`key_grid` from phase 0 generalises to the two labelled columns at the widths
of SPEC §3.5. Palette results are sessions and hosts, never settings, with the
population of SPEC §2.7. Config panel entries wrap to two lines rather than
clip.

## Phase 7, themes (2 to 3 days)

Nine palettes times five new slots, plus SPEC §5.3 for `terminal` and darkened
severity hues for `light`. A screenshot per theme per screen, reviewed side by
side. Needs phase 1's slots and is best done once the screens are stable.

## Phase 8, divergence moves out (mostly done)

`essh diff <tag>` ships and prints what the dashboard used to show. `essh why`
already did. The `Fleet` tab, the `DIVERGE` column, the groups panel and the
`p` / `P` / `R` split are gone; `divergence::` and its tests are intact,
because the engine is correct and the redesign is about where it appears.

`D` stays, against the first draft of SPEC §8. The handoff's case is about the
default dashboard, and a key you have to know about taxes nobody. Removing it
would strand a working drill-down behind a command the TUI never mentions.

Left: the divergence modal itself is un-redesigned. It is reachable only by
`D` now, so it no longer sits on the path a default user takes, but it is
still v1 chrome behind a v2 screen.

## Phase 9, remote port forwards (3 to 5 days, independent)

SPEC §6.4's two prerequisites in `ssh/mod.rs`. Only then does `add_remote` come
back and the add-forward row grow its second form. Until it does, `R` is
refused with a reason, which is the honest state.

## Risks

| Risk | Mitigation |
| --- | --- |
| The fleet sampler is most of v2 and the handoff does not mention it | Phase 2 is its own phase with its own done-condition, ahead of every screen that reads it |
| 200 hosts times a 5s poll is a self-inflicted denial of service | Concurrency limit, jitter and back-off are phase 2 acceptance criteria, not a later hardening pass |
| Connect-per-poll measured on loopback containers looks free and is not | Measure with added latency, record the number in the module doc |
| Five new theme slots times nine palettes | Phase 1 gates on a slot table rendered in all nine |
| Fixture drift, screens disagreeing again | SPEC §2 is the single source and the demo asserts relations, not literals |
| Deleting divergence to satisfy a screen redesign | Phase 8 moves it to the CLI and keeps the engine and its tests |

## Sequencing

Phase 1 gates everything. Phase 2 gates phases 3 and the CPU column of phase 4.
Phase 4 step 1 (LAST LINE) needs neither and is the earliest visible slice of
v2, so it is worth shipping alone. Phases 8 and 9 are independent throughout.

## The demo

`docs/media/essh-spec-demo.gif`, 150x34, the geometry the reference was
authored at, against the eleven-host fixture fleet. Every rate and listing in it
is collected over live SSH.

```
./scripts/spec-demo.sh            verify, render, optimise, report
./scripts/spec-demo.sh --verify   verification only
```

The script runs `tests/demo_verify.rs` first and refuses to publish a GIF whose
beats do not hold, because a demo that shows a defect is worse than no demo.
The two share their key counts so they cannot drift apart silently.

The GIF covers phases 0 to 3: the hosts table filled from hosts nobody has a
session on, the offline row that explains itself, `r` meaning one thing again,
and the health screen. It grows a beat per phase, and the phase is not done
until its beat is in there.

`scripts/fleet.sh` builds and stages the fixture, and `spec-demo.sh` refuses to
run against a fleet that has drifted from it. That is not fussiness: staging
lives in the containers' running state, a restart loses it, and the first
symptom is `demo_verify` failing on "web-02 is not serving :80" and blaming
the app for a fact about the laptop.
