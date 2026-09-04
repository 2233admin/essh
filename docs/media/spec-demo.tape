# spec-demo.tape — the demo verified against docs/redesign/SPEC.md.
#
# Rendered by scripts/spec-demo.sh, which substitutes @HOME@ and @OUT@ and
# checks the fixture fleet is up first. Do not run this file directly.
#
# Every beat here is also a beat in tests/demo_verify.rs, which drives the same
# binary through the same keys against the same ten containers and asserts the
# spec's rules on the resulting screen text. The GIF is what you look at; that
# test is what verifies it. Change one, change the other — the key counts are
# shared deliberately (DOWNS_TO_EDGE).
#
# The fleet is the fixture from SPEC.md §2: eleven Debian containers on
# 127.0.0.11-21:2222, each with its own hostname, staged by scripts/fleet.sh.
# staging-web-01 is kept stopped, so the offline row is drawn from a probe that
# actually failed, and build-01 runs three named busy loops so the busiest
# panel has something real to collapse. Every rate, listing and process name is
# collected over live SSH.
#
# 150x34 — the geometry the design reference was authored at (SPEC.md §6.1).
# Width/Height are calibrated to produce exactly that with this font, size,
# padding, margin and window bar. Changing any of them changes the grid, and
# tests/demo_verify.rs asserts against the same 150x34, so a change here that
# is not made there records a screen nobody verified.
#
# The spec fixes the grid, not a pixel size. FontSize 11 renders the same
# 150x34 in 40% fewer pixels than 13 did, which is most of a megabyte on a
# recording this long — and GitHub scales a README image down to about 880px
# wide anyway, so the larger render was being thrown away on display.

Output "@OUT@"

Set Shell bash
Set FontFamily "JetBrainsMono Nerd Font Mono"
Set FontSize 11
Set Width 1138
Set Height 589
Set Padding 14
Set Margin 16
Set MarginFill "#05080a"
Set BorderRadius 9
Set WindowBar Colorful
Set Framerate 12
Set TypingSpeed 32ms
Set CursorBlink false
Set Theme { "background": "#0c1418", "foreground": "#c8d4d9", "cursor": "#5fdcff", "black": "#0c1418", "red": "#ff7878", "green": "#5cd989", "yellow": "#f0c060", "blue": "#5fdcff", "magenta": "#b8a8e8", "cyan": "#5fdcff", "white": "#c8d4d9", "brightBlack": "#42555d", "brightRed": "#ff7878", "brightGreen": "#5cd989", "brightYellow": "#f0c060", "brightBlue": "#5fdcff", "brightMagenta": "#b8a8e8", "brightCyan": "#5fdcff", "brightWhite": "#ffffff" }

Hide
Type "export HOME=@HOME@"
Enter
Type "clear"
Enter
Show

# ── 1 · the launcher, and a real shell on web-02 ───────────────────────
Type "@HOME@/essh"
Enter
Sleep 2s
Type "web-02"
Sleep 1500ms
Enter
Sleep 5s

# ── 2 · why this host will turn out to differ ──────────────────────────
# web-02 is serving HTTP; the rest of the fleet listens on 22 alone. The
# shell shows the cause a beat before the fleet screen names it as
# "listening ports".
Type "ss -ltn"
Enter
Sleep 3500ms

# ── 3 · two more hosts, so there is something to compare ───────────────
# Facets are collected per session, so the fleet screen knows what these
# three hosts hold and says "never probed" about the other seven.
Ctrl+A
Sleep 400ms
Type "n"
Sleep 1200ms
Type "edge-01"
Sleep 1s
Enter
Sleep 5s
Ctrl+A
Sleep 400ms
Type "n"
Sleep 1200ms
Type "db-01"
Sleep 1s
Enter
Sleep 5s

# ── 4 · the hosts screen, with readings for hosts nobody is on ─────────
# Eight of the eleven have no session. Before v2 essh could not collect
# metrics for those at all: the collector needs an established handle, so a
# host without a session had nothing but a TCP probe. Every meter here comes
# from a sweep over a warm connection the fleet sampler opened.
Ctrl+A
Sleep 400ms
Type "d"
Sleep 1200ms
Type "2"
Sleep 5s

# ── 5 · the row that has no readings, and says why ─────────────────────
# staging-web-01 is a container that is down. Its five measurement columns
# become one explanation carrying the key that fixes it, rather than five
# blanks or five confident zeroes.
Down
Sleep 500ms
Down
Sleep 500ms
Down
Sleep 1500ms

# ── 6 · `r` means one thing again ──────────────────────────────────────
# v0.3.3 advertised `r` as probe and performed a reload that discarded every
# measurement. It spent one release refusing to act. It now retries the host
# under the cursor and clears the sampler's back-off, because doing only the
# first would look like the key half worked.
Type "r"
Sleep 2500ms

# ── 7 · health: what to look at, in words ──────────────────────────────
# Screen 3 was fleet configuration consensus — an agreement percentage and a
# facet grid. It is now the attention list, the cards, the busiest processes
# across the whole fleet, and reachability. The offline host is in the
# attention list because unreachable needs attention, which is the entry the
# handoff drops.
Type "3"
Sleep 5s
Down
Sleep 1200ms
Up
Sleep 1200ms

# ── 8 · the drill-down is still there, behind a key ────────────────────
# Configuration divergence leaves the default dashboard and keeps its modal.
# `essh diff <tag>` answers the same question outside the TUI.
Type "2"
Sleep 1s
Type "D"
Sleep 6s
Escape
Sleep 1s

# ── 9 · port forwards that report what actually bound ──────────────────
# The panel used to say "Active" with nothing listening. The first forward
# binds; the second asks for the same port and fails with the reason.
Type "1"
Sleep 800ms
Enter
Sleep 2s
Ctrl+A
Sleep 400ms
Type "p"
Sleep 1500ms
Type "a"
Sleep 1s
Type "L:18080:localhost:80"
Sleep 800ms
Enter
Sleep 3s
Type "a"
Sleep 800ms
Type "L:18080:localhost:80"
Sleep 800ms
Enter
Sleep 4s
Escape
Sleep 1s

# ── 10 · the keymap, where every binding is one whole cell ─────────────
# `F4 forwards` used to render as "F4 /" then "forwards" — a binding that does
# not exist, printed by the screen whose job is to teach the bindings.
Ctrl+A
Sleep 400ms
Type "?"
Sleep 3s
Down Down Down Down Down Down Down Down Down Down
Down Down Down Down Down Down Down Down Down Down
Down Down Down Down Down Down
Sleep 3500ms
Escape
Sleep 1s

Hide
Type "q"
Sleep 500ms
