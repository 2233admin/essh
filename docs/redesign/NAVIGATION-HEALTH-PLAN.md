# Simpler navigation and a full health dashboard

Status: implemented on `redesign/v3-tmux`. The sections below preserve the design and acceptance criteria.

Delivered: shared F1–F6 navigation, clickable controls and session tabs, a full
responsive health dashboard, visibility-scoped collection, contextual help,
stable restored session numbers, and live binding refresh.

Validation: Rust tests and strict Clippy; isolated SSH/tmux PTY coverage for
navigation, real health data, mouse controls, reserved-key forwarding, collector
cleanup, reconnect, restore, and remote-close failure handling. Rendering checks
cover compact and wide terminals. The HTML reference was checked in Chromium.
Native Ghostty mouse gestures still require a manual check.
Base: `redesign/v3-tmux`, following the F2 health-strip shortcut.

## Outcome

Provide a complete current-host health dashboard, easy session switching by mouse
or keyboard, a direct route to the main menu, and visible navigation help. Normal
use should not require learning the difference between local and remote tmux
prefixes. Keep tmux's persistent sessions and the existing SSH transport.

The main menu is the existing Hosts screen, expanded with clear navigation and
session actions. The full health dashboard is for the selected host/session;
a fleet-wide dashboard is outside this change.

## 1. One navigation model

Use the same labels and actions in the terminal status bar, app footers, mouse
controls, and help. Function keys perform actions directly, without a prefix.

| Key | Action | Behavior |
| --- | --- | --- |
| F1 | Help | Open contextual help; F1 or Esc returns to the previous view. |
| F2 | Health | Open the full dashboard for the current session; F2 or Esc returns to its terminal. |
| F3 | Sessions | Open the session switcher; F3 or Esc closes it. |
| F4 | Main menu | Return directly to Hosts, preserving running sessions. |
| F5 | Previous session | Select the preceding SSH session, wrapping at the start. |
| F6 | Next session | Select the next SSH session, wrapping at the end. |

Additional rules:

- F5/F6 skip the Hosts window and any helper windows/panes. With one session,
  stay on it. With none, show “No sessions — choose a host to connect”.
- In Health, F5/F6 and session-tab clicks keep the dashboard open and change the
  selected session. Clear the old host's display before showing the new data.
- F2 on Hosts opens Health for the last active session. If there is none, keep
  Hosts visible and show “Connect to a host to view health”.
- Help and the switcher preserve the view they were opened from. F4 always goes
  to Hosts. Navigation keys inside popups must work; don't rely on tmux root
  bindings receiving keys consumed by a popup.
- Esc means close/back in essh views. In the terminal it remains a remote key,
  so editors and shell applications continue to receive it normally.
- q on Hosts detaches essh. It does not terminate remote jobs. Label this
  “Quit essh — keep sessions running”. Closing a remote session is a separate,
  clearly named action with confirmation.
- Keep existing Ctrl+A shortcuts as compatibility aliases. Put them in the
  advanced help section. F2 changes from the strip to the full dashboard;
  the dashboard offers a “Show beside terminal” action for the optional strip,
  with Ctrl+A then lowercase m retained as its advanced shortcut.
- Reserve F1–F6 for essh. Document this and provide a “Send next key to terminal”
  action, with an advanced binding, for remote programs that need those keys.
  Verify the actual key sequences in Ghostty before finalizing the defaults.

### Session switcher

Show number, host, current command, connection state, and active selection.

- Up/Down or j/k selects a row; Enter switches.
- 1–9 jumps directly while the switcher is open. More sessions remain available
  through scrolling and filtering.
- Click selects a row; double-click or the explicit Switch button activates it.
- “New session” opens the host picker, even when that host already has sessions.
- Distinguish sessions on the same host with stable IDs and visible numbers.
- Persist visible session ordering/numbering alongside saved IDs so restart does
  not unexpectedly reshuffle the shortcuts.
- A disconnected session remains selectable and offers Reconnect. Never infer
  that its remote jobs are alive simply because its local record exists.

## 2. Mouse support and visible controls

Enable mouse handling in the dedicated local tmux server and handle mouse events
in ratatui views. Currently tmux mouse is off and `ui::key()` drops mouse events.
Both layers need changes.

- Click a session tab in the bottom bar to activate it immediately.
- Click Main menu, Health, Sessions, or Help in the bar to invoke that action.
- Click a host row to select it; double-click or Connect opens it. A single click
  must not unexpectedly create a connection or prompt for credentials.
- Click a pane to focus it and drag a pane divider to resize it.
- Mouse wheel scrolls the list or panel under the pointer. Preserve expected
  terminal scrollback/copy behavior and document Ghostty's selection override.
- Expose the same actions through keyboard focus: Tab/Shift+Tab moves between
  controls or dashboard panels, with a visible focus marker.
- Compute click regions from the rendered layout. Do not hard-code coordinates.
- Keep active session and keyboard focus visually distinct, with labels as well
  as color. Long names truncate within their tab, without covering controls.
- When tabs overflow, show an overflow control opening Sessions. At narrow widths
  shorten labels and use the Sessions control instead of clipping navigation.

The terminal keeps a persistent navigation/status bar. App views also have a
contextual footer. Reserve space for navigation before optional metrics.

Example terminal bar:

    [F4 Main] [1 web-01] [2 db-01] [Sessions F3] … [F2 Health] [F1 Help]

Example host-list footer:

    ↑↓ Select   Enter Connect   a Add   e Edit   / Filter   F1 Help   q Quit essh

Example health footer:

    Tab Panel   ↑↓ Scroll   F5/F6 Session   F2/Esc Terminal   F4 Main   F1 Help

## 3. Full health dashboard

F2 opens a dashboard occupying the available terminal area, with the session
identity and navigation visible. The remote terminal keeps running underneath.
Implement it as an essh-managed full-size tmux popup/view, using a shared action
router so function keys and clicks behave consistently inside it. Prototype the
popup's handling of the persistent tmux bar; if it covers the bar, render the
same navigation controls inside the dashboard. Navigation must remain visible.

### Layout and data

| Section | Content |
| --- | --- |
| Header | Host, session number, connection state, OS, uptime, sample age |
| CPU | Total usage, per-core usage, recent history, load averages |
| Memory | Used/total/available, swap, recent history |
| Network | RX/TX rates and history, interface selection, direct-connect latency where available |
| Storage | Mounted filesystems, used/total capacity; I/O rates where supported |
| Processes | PID, command name, CPU and memory; sortable and scrollable |
| Peers | TCP peer RX/TX rates with clear units and collection limits |

Keep the current square borders, cyan selection, green utilization/RX and cyan TX.
Use readable labels and values, not color alone. The dashboard is read-only;
process termination is outside its scope.

- At 120+ columns: a multi-column overview with graphs and tables visible.
- At 80–119 columns: stack panels, keeping the header and footer visible.
- Below 80 columns or at short heights: use a scrollable compact layout with
  panel tabs; never require a 100-column terminal to access Health.
- Show loading, unavailable, unsupported and stale values explicitly. Include
  an actionable reason where collection fails. Do not substitute zero for missing
  data, and do not label command-execution time as network RTT.
- A disconnect freezes the last sample with its age and a clear disconnected
  banner. Offer Reconnect and Main menu. Clear data when switching hosts.

### Collection

Extend `src/tmux/metrics.rs` over the existing OpenSSH master connection. Reuse
applicable parsers and metric types from `src/monitor/`; its existing collector
uses russh, so it cannot simply be dropped into the new OpenSSH transport.

- Model consumer state explicitly: status only, strip visible, dashboard visible.
  The current sampler only recognizes a strip pane.
- Use one sampler per session, shared by its status bar, strip and dashboard.
- Collect fast metrics about once a second while the strip/dashboard is visible.
  Collect slower filesystem/process data at a separate cadence, around 3–5 seconds.
- Return to light sampling when detailed views close or no client is attached.
  Use cleanup guards plus an expiring visibility lease so a crashed popup cannot
  leave expensive sampling enabled forever.
- Retain bounded commands, no automatic new authentication connections, counter
  reset handling, and per-metric availability. Prioritize Linux; unsupported
  platform metrics must degrade independently without breaking navigation.

## 4. Help and discoverability

Create a shared action registry containing action ID, label, key, applicable
views, and description. Generate bindings, footer hints and the in-app key table
from it. This prevents the implementation and instructions drifting apart.

F1 opens help with these sections:

1. **Getting around** — the six main keys, mouse actions, and how to return.
2. **Sessions** — create, switch, reconnect, close, and quit without killing jobs.
3. **Health** — full dashboard, optional side strip, panels, sampling and units.
4. **Splits and terminal keys** — clearly distinguish same-host remote panes
   from different-host local sessions; describe focus and resize controls.
5. **Advanced tmux** — Ctrl+A compatibility shortcuts, remote Ctrl+B, reserved
   function keys, sending a reserved key through, and copy/scroll behavior.

Use precise key notation: “F2”, or “Ctrl+A, release, then lowercase m”. Never
capitalize a character key in a way that implies Shift. Help scrolls, shows its
close key, and returns to the originating view. Relevant controls remain visible
without opening help. Update README and the HTML reference from the final map.

## 5. Delivery order

| Phase | Work | Completion check |
| --- | --- | --- |
| 1 | Shared action registry and navigation routing; F1/F3/F4/F5/F6; explicit back behavior | Navigate among hosts, sessions and help using direct keys, with no prefix knowledge |
| 2 | Mouse handling, clickable tabs/actions, list selection, focus and overflow | Complete connect → switch → main menu using mouse alone |
| 3 | Metrics model, parser reuse and visibility-aware sampling | Real readings, independent unavailable states, no duplicate collectors or stale-host data |
| 4 | Full dashboard, F2 toggle, responsive panels and side-strip action | Open/close Health at 80 and 120 columns; switch hosts while Health remains open |
| 5 | Contextual help, documentation, regression tests and live-session binding refresh | Every documented action matches behavior; existing SSH work survives the upgrade |

Each phase should produce a runnable commit on the redesign branch. Refresh
bindings in the running local tmux server after a successful build. Do not kill
active sessions just to reload navigation. This plan supersedes the earlier
handoff's restrictions on the dashboard, function keys, and mouse support.

## 6. Acceptance tests

Extend the isolated SSH/tmux regression harness and terminal rendering checks:

- F1–F6 work from terminal, dashboard, host list, help and session switcher,
  according to the rules above; cover empty and single-session cases.
- F2 opens the full dashboard and returns to the exact original terminal/pane.
- F4 returns to the main menu without disconnecting or losing shell state.
- Keyboard and mouse activate the same session IDs, including duplicate hosts,
  more than nine sessions, overflow tabs, and restored session ordering.
- Mouse clicks, double-clicks, wheel scrolling and pane resizing work in Ghostty;
  retain keyboard-only coverage in a real pseudo-terminal.
- 60×20, 80×24, 120×40 and wide layouts keep navigation/help accessible and avoid
  overlap. Help remains usable while a remote full-screen program is running.
- Dashboard data changes with the selected host; failed metrics never look like
  measured zeros. Closing/killing the view stops detailed sampling.
- Dropped SSH connections, missing tools, unknown remote state, and unsuccessful
  remote termination retain honest status and recoverable session records.
- Test reserved-key forwarding and ordinary terminal input, especially Esc,
  Ctrl+C and remote tmux's prefix. Do not break editors to simplify navigation.
- Build, format, strict Clippy, unit tests and the live SSH regression suite pass.
