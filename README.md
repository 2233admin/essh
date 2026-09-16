# essh

**An SSH workspace with persistent sessions and live health dashboards.**

Keep remote shells running across connection drops, switch between hosts with
keys or the mouse, and check system health without leaving your terminal.
ESSH uses OpenSSH for connections and tmux to keep your work alive.

## See it in action

[![ESSH admin workflow: Linux commands, health dashboards and session switching](docs/media/essh-demo.gif)](https://github.com/matthart1983/essh/releases/download/v0.4.0/essh-v3-demo.mp4)

**[Watch the 38-second demo](https://github.com/matthart1983/essh/releases/download/v0.4.0/essh-v3-demo.mp4)** ·
[Download the browser replay](https://github.com/matthart1983/essh/releases/download/v0.4.0/essh-v3-replay.html)

Inspect disks with `df` and `lsblk`, find busy processes with `ps`, sample system
activity with `vmstat`, and check sockets and interfaces with `ss` and `ip`.
The demo shows the side, half and full health views, session switching, help,
and a dropped connection recovering the same remote shell.

## Install

Download a binary for **Linux or macOS**, on **x86_64 or ARM64**, from
[Releases](https://github.com/matthart1983/essh/releases/latest). Extract the archive
and put `essh` somewhere on your `PATH`.

You need **tmux 3.3+ and OpenSSH locally**, and **tmux on each remote host**.
Detailed health metrics are available for Linux hosts.

Or build from source with Rust:

```sh
git clone https://github.com/matthart1983/essh.git
cd essh
cargo build --release
./target/release/essh
```

## Connect to a host

ESSH reads hosts from `~/.ssh/config`, its own configuration, and its host cache.
For example:

```sshconfig
Host web-01
    HostName 192.168.1.20
    User deploy
    IdentityFile ~/.ssh/id_ed25519
```

Run `essh`, select a host, and press **Enter**. Use **/** to filter hosts and
**Esc** to clear the filter. Press **a** to add a host or **e** to edit one.
OpenSSH handles authentication and host-key checking.

## Find your way around

| Key | Action |
| --- | --- |
| **F1** | Open or close help |
| **F2** | Cycle terminal → side health → half health → full health → terminal |
| **F3** | Open Sessions; select a session or press **n** to start another |
| **F4** | Return to the main menu while sessions keep running |
| **F5 / F6** | Switch to the previous / next session |
| **h** on Hosts | Open health for the selected host |
| **r** after a connection drop | Reconnect |
| **q** on Hosts | Leave ESSH while remote work keeps running |

Click session tabs and navigation controls to switch views. Click a row to select
it; double-click to connect or switch. Use the mouse wheel to scroll, click panes
to focus them, and drag dividers to resize them.

If your keyboard uses the function row for brightness or volume, hold **Fn**
while pressing an F-key, or enable standard function keys in your keyboard settings.
The navigation controls are also clickable, and **F1** lists the available keys.

## Your shell and theme

Remote sessions start the remote account's configured login shell and load its
normal startup files—zsh, bash, fish, or whichever shell that account uses.
Your prompt, aliases and shell key bindings come from that remote account.
Existing shells retain their running state when you reconnect.

ESSH uses your terminal's colors by default. The shell always keeps the terminal's
own foreground and background, even when you choose a different dashboard theme:

```sh
essh --theme terminal
essh --theme light
essh --theme dark
```

To save a preference, set `theme = "terminal"` (or another theme name) at the top
of `~/.essh/config.toml`. Apply it to an open workspace with
`essh tmux refresh`; SSH sessions keep running. Your terminal emulator controls
the font and font size.

## Choose your health view

Press **F2** to cycle through:

| View | What you see |
| --- | --- |
| Terminal | Your shell with the session and navigation bars |
| Side health | A narrow strip with traffic, CPU, memory, disk and latency |
| Half health | Your shell and a dashboard sharing the terminal |
| Full health | Network traffic, CPU cores, memory, storage, peers and processes |

The dashboards use thin meters and live readings. Unavailable readings show **—**;
disconnected hosts are clearly marked. Narrow terminals skip the side layouts.

In the dashboard:

- **Tab** selects a panel; **arrows** and the **mouse wheel** scroll.
- **i** selects a network interface; **c / m** sorts processes by CPU or memory.
- **Left / Right** browses hosts, including hosts without a connected session.
- **Esc** closes full health and restores the previous side view.
- **F2** from full health returns to the terminal with side health off.

For direct access, press **Ctrl+A**, release it, then **h** for full health or
**m** to cycle side health → half health → off.

## Work across sessions and panes

Use **F3**, then **n**, to open another independent session. Switch with
**F5 / F6**, the Sessions picker, or the mouse. **F4** returns to the host list
without ending your sessions.

Session IDs survive local restarts. Reconnecting reattaches the remote shell
when it still exists; if it has ended, ESSH starts a new shell. A disconnected
session's remote state remains unverified until you reconnect.

**Ctrl+A** controls local tmux. **Ctrl+B** controls tmux on the remote host.
Press the prefix, release it, then press the next key:

| Keys | Action |
| --- | --- |
| **Ctrl+B**, then **%** | Split the remote terminal side by side |
| **Ctrl+B**, then **"** | Split the remote terminal top and bottom |
| **Ctrl+B**, then an **arrow** | Move between remote panes |
| **Ctrl+A**, then **v** | Send the next key directly to the remote application |
| **Ctrl+A**, then **x** | Terminate a remote session after confirmation |

F1–F6 are reserved for ESSH. Use **Ctrl+A**, then **v**, when a remote application
needs one of those keys.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
ESSH_BIN=target/release/essh python3 scripts/test-tmux-v3.py
```

The live regression check uses an isolated SSH server and tmux sockets.
[Demo recording and verification](docs/demo-v3.md) explains how to reproduce the tour.

To contribute, create a branch, make your change, run the checks, and open a pull request.

## License

[MIT](LICENSE)
