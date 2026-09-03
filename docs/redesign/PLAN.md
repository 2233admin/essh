# ESSH TUI redesign — implementation plan

Companion to `SPEC.md`. Phase 0 is done; phases 1–7 are sized against the code
as it stands at `209d946` plus phase 0.

Ordering rule: nothing that renders a number ships before the data behind it
exists. The original handoff's biggest risk is three screens whose panels look
like layout work and are collection work.

## Phase 0 — the seven defects (done)

All in the working tree. `cargo test`: 481 pass, `cargo clippy --all-targets`:
clean.

| # | Fix | Files | Pinned by |
| --- | --- | --- | --- |
| 1 | `note()` no longer repeats the ratio the column prints | `divergence/mod.rs` | `a_consensus_ratio_is_printed_exactly_once` |
| 2 | `bind_forward` returns the `io::Error`; `ForwardState` replaces `active: bool`; forwards are actually started and their listeners released on delete | `portfwd/mod.rs`, `main.rs`, `tui/portfwd_view.rs`, `tui/mod.rs` | `a_forward_that_did_not_bind_is_never_shown_as_active`, `a_bind_that_fails_says_why`, `the_summary_counts_only_forwards_that_bound` |
| 3 | `p` probe host, `P` probe fleet, `R` reload; `apply_probe_state` puts measurements back after a reload; `r` reports the split for one release | `main.rs`, `tui/dashboard.rs` | `no_advertised_command_does_nothing` |
| 4 | Add form gets its own rows, panel widened to 100, reasons kept short | `tui/portfwd_view.rs`, `main.rs` | `the_add_form_format_is_never_clipped` |
| 5 | `key_grid` lays bindings out as indivisible cells | `tui/help.rs` | `no_binding_is_split_across_rows` (5 widths) |
| 6 | `format::count` agrees the noun with the number | `format.rs`, `divergence/`, `tui/` | `counts_agree_with_their_noun` |
| 7 | `discard_unconnected_sessions` — no path leaves a session on "Connecting"; dashboard Enter defers like the launcher | `main.rs` | `a_session_that_never_connected_does_not_survive_the_attempt` |

### Found while building the demo

Recording the demo meant running every screen against a real ten-host fleet
and asserting the spec's rules on the result. That is a different test from
any unit test in the tree, and it found seven more defects — three of them in
work done earlier the same day.

| Defect | Where |
| --- | --- |
| The FLEET table sorted by divergence while the cursor walked the configured order, and drew no selection marker at all — so `D` opened the card for a host nobody had pointed at. The demo tape worked around it with "three presses, not one" | `tui/mod.rs`, `tui/dashboard.rs` |
| A reload rebuilt the list cache-first, so indices shifted under the cursor and the selection moved to a different machine. The next Enter would connect to it | `tui/mod.rs` |
| `and 1 other hosts` — the same defect as `1 peers`, in a string the first pass did not cover | `divergence/mod.rs` |
| `2 of 10 hosts hold every outlier` counted seven never-probed hosts as though they were fine (SPEC §1) | `tui/dashboard.rs` |
| Facets at 100% drew full-width meters that merged into one solid rectangle across rows, burying the three that diverged. SPEC §2.1 says agreement needs no graphic | `tui/dashboard.rs` |
| Diverging facets took their colour from a ramp read at `1 - agreement`, so 86% landed at the cold end and the diverging facets rendered quieter than the agreeing ones | `tui/dashboard.rs` |
| `Esc / Ctrl+pClose palette` — a key label that filled its column exactly and ran into its description | `tui/help.rs` |

Two of the phase 0 fixes were also incomplete, and only the live run showed it:
the help screen still taught `r  Refresh hosts` (an edit lost when a patch
aborted), and the timing benches were still flaky at best-of-five under a
loaded parallel suite — they are now `#[ignore]`d and run deliberately.

### Also found on the way



- **Ties were decided by string order.** `score_categorical` named a consensus
  even when the top value did not lead, so `db-01` vs `db-02` reported one as
  the outlier because its string sorted later. A tie now yields no consensus and
  blames nobody. This overrides a documented decision — the superseded test
  `a_two_host_split_names_exactly_one_side` asserted the old behaviour for
  determinism. Determinism is preserved and asserted; correctness is new.
- **The timing benches were flaky under parallel `cargo test`** (17.75ms against
  a frame budget, 6ms alone). They now take the best of five runs.

### The demo, and what verifies it

`docs/media/essh-spec-demo.gif` — 150×34, the geometry the reference was
authored at, against the ten-host fixture fleet. Every hash, rate and listing
in it was collected over live SSH.

```
./scripts/spec-demo.sh            verify, render, optimise, report
./scripts/spec-demo.sh --verify   verification only
```

The script refuses to publish a GIF whose beats do not hold: it runs
`tests/demo_verify.rs` first, which drives the same binary through the same
keys against the same fleet and asserts the spec's rules on the resulting
screen text. A screen is text before it is pixels, and "a count agrees with its
noun" is an assertion rather than an impression — so the GIF is what a person
looks at and that test is what verifies it. The two share their key counts
(`DOWNS_TO_EDGE`) so they cannot drift apart silently.

Rules it enforces on every frame, not just the one that shows them: no count
disagrees with its noun, no binding is split by wrapping, `r` is never
advertised as probe, no consensus ratio is printed twice, and no lone probed
host is reported as full consensus. Per-beat it checks the unprobed wording,
the probe/reload split, the fleet cursor, that `D` opens the card for the host
under it, that the add-forward format is whole, and that a forward reports
`active` only when something bound.

What the GIF cannot show yet: the redesign itself. Phase 0 is behaviour and
copy, so the demo is the current screens telling the truth — not the panels,
meters and columns of phases 1–6.

## Phase 1 — design system (2–3 days)

Nothing renders differently at the end of this phase. It is the foundation the
other five screens assume.

1. Add six slots to `theme::Theme`: `severity_warn`, `severity_worst`,
   `panel_alt`, `terminal_bg`, `footer_bg`, `meter_track`. Fill them in all nine
   palettes. `no_screen_paints_from_a_hardcoded_colour` already fails the build
   if a screen bypasses them.
2. `design::meter_ramped(fraction, width, ramp)` — a run of `█` coloured by
   **position along the scale**, not by value, with `meter_track` behind.
   Falls back to `design::meter`'s single colour when
   `Theme::defers_to_terminal()`. Test both branches.
3. Split `status_warn` at the call sites: 1 divergence → `severity_warn`, worst
   offender → `severity_worst`.
4. Global header and footer as shared widgets, with the active tab **derived**
   from `AppView` + `DashboardTab` rather than passed in.

**Done when** a table of every slot renders in all nine themes and the terminal
theme emits no RGB.

## Phase 2 — hosts and fleet (3–4 days)

The only screens whose data already exists (`divergence`, `fleet`,
`format::tag_chips`, `design::sparkline`).

1. Hosts table to the spec's columns and severity rules. `◌` and the word
   `unprobed` for no facets; `·` for at-consensus.
2. Selected row: style + `▌` in the gutter, no column shift.
3. Groups panel per §2.2, including `—` for no majority (phase 0 shipped the
   engine and the column; this is the meter and the copy).
4. Fleet: headline, agreement meter, facet grid diverging-first, verdict,
   outliers. All counts scoped and labelled per §2.4.
5. Header counts from §1 — the three-way facet partition plus reachability.

**Done when** a fixture-driven render test asserts the §2 numbers on both
screens, and no count on either disagrees with another.

## Phase 3 — monitor (5–8 days, the long pole)

Collection first, panels second.

1. **Per-flow net** (§5.1). Extend `monitor::platform` with a socket-table
   command, `monitor::parser` with a parser, and `HostMetrics` with
   `Vec<Flow>`. Golden-file tests as for `parse_netstat_ib`.
2. Degradation: no root → own processes only, labelled; no `ss` → aggregate
   only, labelled. Both need a render test.
3. **Fleet median CPU** (§5.2): a sampler over probed hosts on the probe cycle.
   `peers —` until it reports.
4. Panels 1–3 (cpu, memory, disk) — data exists, layout only.
5. Panel 4 (net): mirrored histories + flow table.
6. Panel 5 (processes): collapse identical siblings with `×N` and summed RSS,
   `g` ungroups. View-level; `ProcessInfo` already has what it needs.

**Done when** the flow table's rates sum to the headline on a live host, and
the panel says which of the three collection modes it is in.

## Phase 4 — session, strip, files, forwards (3–4 days)

1. Session chips with per-host severity dots; meta line with
   `N of M forwards active` from real state.
2. Strip: the four small panels at the same chrome.
3. Files: pre-shortened LTR paths — **no `direction` trick**, it reorders the
   neutral `/` and displays a path that is not the real one. Collision
   highlighting, transfer rate and ETA.
4. Forwards panel to the phase 0 state model; add-forward row outside the
   border (shipped); title says local-only until phase 7.

## Phase 5 — help, palette, config (2 days)

`key_grid` from phase 0 generalises to the two labelled columns. Palette card
and the `~/.essh` config grid. The dashboard list documents `p`/`P`/`R`.

## Phase 6 — themes (2–3 days)

Nine palettes × six new slots, plus §6.3 for `terminal` and darkened severity
hues for `light`. A screenshot per theme per screen, reviewed side by side.

## Phase 7 — remote port forwards (3–5 days, independent)

Two prerequisites, both in `ssh/mod.rs`:

1. `SshSession.handle` is `Arc<Handle<_>>`; `tcpip_forward` needs `&mut`. Move
   to `Arc<Mutex<Handle>>` or hold a dedicated handle for forwarding. The
   metrics sampler and the shell channel share this handle — measure the
   contention before choosing.
2. `server_channel_open_forwarded_tcpip` currently accepts the channel and
   drops it. It has to route to the registered forward and proxy to the local
   target.

Only then does `add_remote` come back. Until it does, `R` is refused with a
reason (phase 0), which is the honest state.

## Risks

| Risk | Mitigation |
| --- | --- |
| Per-flow attribution needs root; most fleets don't give it | Three declared modes (§5.1), each labelled on the panel. Decide before building the panel, not after. |
| The `Arc<Handle>` change (phase 7) touches the shell and the sampler | Independent phase, behind the refusal that already ships |
| Six new theme slots × nine palettes is where a redesign quietly regresses | Phase 1 gates on a slot table rendered in all nine |
| The divergence modal stays un-redesigned behind `D` (§9) | Decide scope before phase 2 review, not during |
| Fixture drift — screens disagreeing again | §2 is the single source; render tests assert against it |

## Sequencing

Phases 1 → 2 → 4 → 5 are one thread. Phase 3's collection work (steps 1–3)
starts in parallel with phase 1 and gates only its own panels. Phase 6 needs
phase 1's slots and is best done once the screens are stable. Phase 7 is
independent throughout.
