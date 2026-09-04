# spec-demo.tape — the demo verified against docs/redesign/SPEC.md.
#
# Rendered by scripts/spec-demo.sh, which substitutes @HOME@ and @OUT@ and
# checks the fixture fleet is up first. Do not run this file directly.
#
# Every beat here is also a beat in tests/demo_verify.rs, which drives the same
# binary through the same keys against the same containers and asserts the
# spec's rules on the resulting screen text. The GIF is what you look at; that
# test is what verifies it. Change one, change the other — the key counts are
# shared deliberately (DOWNS_TO_EDGE), and the two orders match beat for beat.
#
# What the first cut of this tape got wrong, since the same traps are still
# here. It opened on eight seconds of a scratch path being typed at a bare
# prompt, followed by a Debian MOTD — fifteen seconds before anything about
# essh was on screen. It lingered on freshly-opened shells that were 85% empty,
# because a container nobody has typed into has nothing in it. It showed the
# health screen three times. It ended on a blank frame. And it never showed the
# sessions screen at all, which is the screen v2 opens on and the whole point
# of the rearrangement — the beat existed in demo_verify and not here, which is
# exactly the drift the paragraph above warns about.
#
# So: the launch is hidden, every shell is given real work before it is shown,
# and no screen appears twice.
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
# The launch itself is hidden. It is a scratch path being typed at a prompt,
# which says nothing about essh and was the first eight seconds of the frame.
Type "@HOME@/essh"
Enter
Sleep 2500ms
Show

# ── 1 · the launcher ───────────────────────────────────────────────────
Type "web-02"
Sleep 1200ms
Enter
Sleep 3500ms

# ── 2 · a real shell, and the reason this host will differ ─────────────
# web-02 serves HTTP; most of the fleet listens on 22 alone. The shell shows
# the cause a beat before `D` names it as "listening ports".
Type "ss -ltn"
Enter
Sleep 2500ms

# ── 3 · two more sessions ──────────────────────────────────────────────
# The login banner is real and it is shown once, on the first host. Showing it
# three more times is twenty seconds of Debian's copyright notice, which is
# what the first cut of this tape spent them on.
Ctrl+A
Sleep 300ms
Type "n"
Sleep 800ms
Type "edge-01"
Sleep 700ms
Enter
Sleep 3s
Type "clear"
Enter
Sleep 600ms
Ctrl+A
Sleep 300ms
Type "n"
Sleep 800ms
Type "db-01"
Sleep 700ms
Enter
Sleep 3s
Type "clear"
Enter
Sleep 600ms

# ── 4 · give a session something to be doing ───────────────────────────
# Real work, not a staged string: a checksum sweep that reads every file under
# /usr, so the sessions screen's RUNNING, CPU and LAST LINE columns have
# something true to report and the health screen's busiest list has something
# to find. `scripts/fleet.sh` installs it; the demo starts it, because a
# fixture that starts jobs nobody asked for lies about what the host was doing.
#
# Three attempts got here. `| tail -1` buffers, so nothing streamed. A single
# `tar` over /usr finishes in under a second on this machine, and the screen it
# exists for is eight seconds away. A loop of short commands runs forever and
# still reports near-zero CPU, because `ps aux` gives %CPU as a lifetime
# average and every child is a fraction of a second old. One long-lived
# process is the only shape that satisfies all three columns.
Type "nightly-verify /usr"
Enter
Sleep 3s

# ── 5 · the sessions screen, which v2 opens on ─────────────────────────
# Sessions are the primary object; hosts are how you get more of them. LAST
# LINE is the column the screen exists for: what a detached session is doing,
# without attaching to it.
Ctrl+A
Sleep 300ms
Type "d"
Sleep 900ms
Type "1"
Sleep 4500ms

# ── 6 · quick connect takes the digits while it holds focus ────────────
# The handoff wants `1` to switch tab, jump to session 1 and open a host, all
# on this screen. Tab is what makes the third one unambiguous.
Tab
Sleep 2500ms

# ── 7 · broadcast: type once, many hosts ───────────────────────────────
# The only multi-host action v2 keeps, in place of the whole fleet-consensus
# screen.
Tab
Sleep 800ms
Type "t"
Sleep 400ms
Type "role=web"
Sleep 600ms
Tab
Sleep 400ms
Type "uptime"
Sleep 800ms
Enter
Sleep 3s

# Focus is sticky, so there has to be a way back to the table. Without one,
# Enter here sends another broadcast rather than attaching.
Escape
Sleep 1200ms

# ── 8 · the hosts screen, with readings for hosts nobody is on ─────────
# Eight of the eleven have no session. Before v2 essh could not collect
# metrics for those at all: the collector needs an established handle, so a
# host without a session had nothing but a TCP probe. Every meter here comes
# from a sweep over a warm connection the fleet sampler opened.
Type "2"
Sleep 4s

# ── 9 · walk the cursor to the host the drill-down will explain ────────
# Three presses, and the marker is on the row it says it is. The row below it
# is staging-web-01, a container that is actually down: its five measurement
# columns become one explanation carrying the key that fixes it, rather than
# five blanks or five confident zeroes.
Down
Sleep 400ms
Down
Sleep 400ms
Down
Sleep 1800ms

# ── 10 · `r` means one thing again ─────────────────────────────────────
# v0.3.3 advertised `r` as probe and performed a reload that discarded every
# measurement. It spent one release refusing to act. It now retries the host
# under the cursor and clears the sampler's back-off, because doing only the
# first would look like the key half worked.
Type "r"
Sleep 2500ms

# ── 11 · health: what to look at, in words ─────────────────────────────
# Screen 3 was fleet configuration consensus — an agreement percentage and a
# facet grid. It is now the attention list, the cards, the busiest processes
# across the whole fleet, and reachability.
Type "3"
Sleep 4s
Down
Sleep 1000ms

# ── 12 · the drill-down is still there, behind a key ───────────────────
# Configuration divergence leaves the default dashboard and keeps its modal.
# `essh diff <tag>` answers the same question outside the TUI.
Type "2"
Sleep 900ms
Type "D"
Sleep 3500ms
Escape
Sleep 800ms

# ── 13 · forwards that report what actually bound ──────────────────────
# The panel used to say "Active" with nothing listening. The first binds; the
# second asks for the same port and fails with the reason; the third is a
# remote forward, which spent a release being refused because the channels it
# needed were being dropped.
Type "1"
Sleep 700ms
Enter
Sleep 1500ms
# A clean pane behind the overlay. The banner is real and was worth showing
# once; five frames of it behind the panel that is the actual subject is not.
Type "clear"
Enter
Sleep 600ms
Ctrl+A
Sleep 300ms
Type "p"
Sleep 1200ms
Type "a"
Sleep 600ms
Type "L:18080:localhost:80"
Sleep 500ms
Enter
Sleep 2200ms
Type "a"
Sleep 600ms
Type "L:18080:localhost:80"
Sleep 500ms
Enter
Sleep 2500ms
Type "a"
Sleep 600ms
Type "R:19090:127.0.0.1:22"
Sleep 500ms
Enter
Sleep 2800ms
Escape
Sleep 800ms

# ── 14 · every binding on one screen ───────────────────────────────────
# `F4 forwards` used to render as "F4 /" then "forwards" — a binding that does
# not exist, printed by the screen whose job is to teach the bindings. Every
# binding is its own grid cell now, and the column budget clears the longest
# of them rather than clipping it.
Ctrl+A
Sleep 300ms
Type "d"
Sleep 900ms
Type "4"
Sleep 4500ms

# ── 15 · the command menu: sessions and hosts, not settings ────────────
Ctrl+P
Sleep 900ms
Type "db"
Sleep 2800ms
Escape
Sleep 600ms

# ── 16 · end where it started: what needs looking at ───────────────────
# A last frame worth pausing on. The first cut ended on the shell it had quit
# back to, which is a blank screen.
Type "3"
Sleep 3500ms

Hide
Type "q"
Sleep 500ms
