#!/usr/bin/env python3
"""Live v3 regression test. Requires Unix, tmux 3.3+, sshd, ssh and ssh-keygen.

Uses a loopback-only sshd, ephemeral keys, private known_hosts and isolated local
and remote tmux sockets. Does not alter the user's configuration or services.
Run after cargo build: python3 scripts/test-tmux-v3.py
"""
import fcntl
import json
import os
from pathlib import Path
import pty
import pwd
import select
import shlex
import shutil
import socket
import struct
import subprocess
import tempfile
import termios
import time
import tomllib

BIN = Path(os.environ.get("ESSH_BIN", Path(__file__).resolve().parents[1] / "target/debug/essh"))


def main():
    for tool in ("tmux", "sshd", "ssh", "ssh-keygen"):
        if not shutil.which(tool):
            raise SystemExit(f"Required fixture tool missing: {tool}")
    if not BIN.exists():
        raise SystemExit("Build essh first with cargo build")
    with tempfile.TemporaryDirectory(prefix="essh-v3-test-") as temp:
        root = Path(temp)
        for directory in (".essh", "local", "remote", "bin"):
            (root / directory).mkdir()
        env = dict(os.environ, HOME=temp, TMUX_TMPDIR=str(root / "local"), TERM="xterm-256color")
        env.pop("TMUX", None)
        remote_env = dict(env, TMUX_TMPDIR=str(root / "remote"))
        for name in ("host_key", "client_key"):
            subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(root / name)], check=True)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        (root / "sshd_config").write_text(f"""HostKey {root}/host_key
ListenAddress 127.0.0.1
Port {port}
PidFile {root}/sshd.pid
AuthorizedKeysFile {root}/client_key.pub
StrictModes no
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
SetEnv TMUX_TMPDIR={root}/remote
LogLevel ERROR
""")
        (root / "known_hosts").write_text(f"[127.0.0.1]:{port} " + (root / "host_key.pub").read_text())
        (root / "ssh_config").write_text(f"""Host *
 UserKnownHostsFile {root}/known_hosts
 StrictHostKeyChecking yes
 IdentityFile {root}/client_key
 IdentitiesOnly yes
""")
        wrapper = root / "bin/ssh"
        wrapper.write_text(f"#!/bin/sh\nexec {shlex.quote(shutil.which('ssh'))} -F {shlex.quote(str(root / 'ssh_config'))} \"$@\"\n")
        wrapper.chmod(0o755)
        env["PATH"] = str(root / "bin") + os.pathsep + os.environ["PATH"]
        (root / ".essh/config.toml").write_text(f"""[[hosts]]
name="fixture"
hostname="127.0.0.1"
port={port}
user="{pwd.getpwuid(os.getuid()).pw_name}"
[[hosts]]
name="offline"
hostname="127.0.0.1"
port=1
""")
        account_shell = pwd.getpwuid(os.getuid()).pw_shell
        if Path(account_shell).name == 'zsh':
            (root / '.zshrc').write_text('export ESSH_SHELL_RC=loaded\n')
            sshd_config = root / 'sshd_config'
            sshd_config.write_text(sshd_config.read_text().replace(
                f'SetEnv TMUX_TMPDIR={root}/remote',
                f'SetEnv TMUX_TMPDIR={root}/remote ZDOTDIR={root}'))
        test_theme = os.environ.get('ESSH_TEST_THEME', 'terminal')
        stale_shell = os.environ.get('ESSH_TEST_STALE_SHELL', '1') == '1'
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 120, 0, 0))
        client = None
        transcript = bytearray()

        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        def launch():
            return subprocess.Popen([str(BIN), "--theme", test_theme], stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=controlling_terminal)

        def drain(seconds=0.1):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                if select.select([master], [], [], 0.05)[0]:
                    try:
                        transcript.extend(os.read(master, 65536))
                    except OSError:
                        break

        def tmux(*args, remote=False, check=True):
            result = subprocess.run(["tmux", "-L", "essh" if remote else "essh-v3", *args],
                                    env=remote_env if remote else env, capture_output=True, text=True)
            if check:
                assert result.returncode == 0, result.stderr
            return result.stdout.strip()

        def cli(*args):
            result = subprocess.run([str(BIN), "--theme", test_theme, "tmux", *args], env=env, capture_output=True, text=True)
            assert result.returncode == 0, result.stderr
            return result.stdout

        def wait_for(predicate, description, seconds=15):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                drain()
                if predicate():
                    return
            raise AssertionError(f"Timed out: {description}\n" + tmux("capture-pane", "-p", check=False) + "\n" + tmux("show-messages",check=False)[:2400])

        def view(target):
            return tmux("capture-pane", "-p", "-t", target, check=False)

        def state(index):
            return tmux("show-window-options", "-v", "-t", f"essh:{index}", "@essh_state", check=False)

        with (root / "sshd.log").open("w") as log:
            server = subprocess.Popen([shutil.which("sshd"), "-D", "-e", "-f", str(root / "sshd_config")], stdout=log, stderr=log)
            try:
                time.sleep(0.3)
                assert server.poll() is None, (root / "sshd.log").read_text()
                client = launch()
                wait_for(lambda: "fixture" in view("essh:0"), "host list")
                assert tmux("show-options", "-gv", "prefix") == "C-a"
                assert tmux('show-window-options', '-gv', 'window-style') == 'default'
                assert tmux('show-window-options', '-gv', 'window-active-style') == 'default'
                style = tmux('show-options', '-gv', 'status-style')
                assert ('#' not in style) if test_theme == 'terminal' else ('#' in style)
                assert tmux('show-options', '-sv', 'extended-keys') == 'on'
                # Simulate a stale remote server whose default shell/command
                # differs from this account. ESSH must honor the account shell.
                if stale_shell:
                    tmux('new-session', '-d', '-s', 'bootstrap', remote=True)
                    tmux('set-option', '-g', 'default-shell', '/bin/sh', remote=True)
                    tmux('set-option', '-g', 'default-command', 'exec /bin/sh', remote=True)
                wait_for(lambda: "USER@ADDRESS" in view("essh:0"), "wide host layout")
                config_before = (root / '.essh/config.toml').read_bytes()
                os.write(master, b't')
                wait_for(lambda: 'Themes ·' in view('essh:0'), 'theme picker opens from Hosts')
                os.write(master, b'\x1b[B\x1b')
                wait_for(lambda: 'Main menu' in view('essh:0'), 'theme preview cancels')
                assert (root / '.essh/config.toml').read_bytes() == config_before
                os.write(master, b't')
                wait_for(lambda: 'Themes ·' in view('essh:0'), 'theme picker reopens')
                # Click Netwatch (second row, below the two-row heading).
                os.write(master, b'\x1b[<0;3;4M\x1b[<0;3;4m')
                wait_for(lambda: tomllib.loads((root / '.essh/config.toml').read_text()).get('theme') == 'netwatch', 'mouse selection saves Netwatch')
                wait_for(lambda: 'Main menu' in view('essh:0'), 'theme apply returns to Hosts')
                assert '#' in tmux('show-options', '-gv', 'status-style')
                # A helper without --theme must pick up the saved choice.
                subprocess.run([str(BIN), 'tmux', 'refresh'], env=env, check=True)
                wait_for(lambda: 'Main menu' in view('essh:0'), 'saved theme survives refresh')
                assert 'netwatch' in tmux('display-message', '-p', '-t', 'essh:0', '#{pane_start_command}')
                os.write(master, b't')
                wait_for(lambda: 'Themes ·' in view('essh:0'), 'restore test theme')
                os.write(master, b'\x1b[H' + (b'\x1b[B' * (2 if test_theme == 'light' else 0)) + b'\r')
                wait_for(lambda: tomllib.loads((root / '.essh/config.toml').read_text()).get('theme') == test_theme, 'keyboard theme selection saves')
                wait_for(lambda: 'Main menu' in view('essh:0'), 'Hosts after keyboard theme selection')

                os.write(master, b"h")
                wait_for(lambda: "Connect to a host" in transcript.decode(errors="replace"), "health without a session")
                os.write(master, b"\x1b")
                drain(.3)
                os.write(master, b"\x1b[15~")
                drain(.3)
                assert tmux("display-message", "-p", "#{window_index}") == "0"
                os.write(master,b"/no-such-host")
                wait_for(lambda: 'no hosts match "no-such-host"' in view("essh:0"), "empty filter feedback")
                os.write(master,b"\x1b")
                wait_for(lambda: "no hosts match" not in view("essh:0"), "Escape clears filter")
                os.write(master,b"a")
                wait_for(lambda: "Add host" in view("essh:0"), "add host form")
                os.write(master,b"extra\t" + pwd.getpwuid(os.getuid()).pw_name.encode() + b"\t127.0.0.1\t\x7f\x7f" + str(port).encode() + b"\r")
                wait_for(lambda: "extra" in view("essh:0") and "Main menu" in view("essh:0"), "save new host")
                saved_config=(root / ".essh/config.toml").read_text()
                assert saved_config.index('name = "fixture"') < saved_config.index('name = "offline"') < saved_config.index('name = "extra"')
                os.write(master,b"e")
                wait_for(lambda: "Edit host" in view("essh:0"), "edit selected added host")
                os.write(master,b"\x1b")
                wait_for(lambda: "Main menu" in view("essh:0"), "Escape cancels edit form")
                cli("open", "fixture")
                wait_for(lambda: state(1) == "connected", "SSH connection")
                assert tmux('show-options', '-gv', 'default-shell', remote=True) == account_shell
                assert tmux('show-options', '-gv', 'default-command', remote=True) == ''
                if stale_shell:
                    tmux('kill-session', '-t', '=bootstrap', remote=True)
                if Path(account_shell).name == 'zsh':
                    tmux('send-keys', '-t', 'essh:1', 'echo SHELL_RC_$ESSH_SHELL_RC', 'Enter')
                    wait_for(lambda: 'SHELL_RC_loaded' in view('essh:1'), 'user shell startup file loaded')
                tmux("send-keys", "-t", "essh:1", "export ESSH_PERSIST_MARKER=works", "Enter")
                drain(0.3)
                first = tmux("show-window-options", "-v", "-t", "essh:1", "@essh_id")
                cli("open", "fixture")
                assert len(tmux("list-windows", "-t", "essh").splitlines()) == 2
                os.write(master, b"\x1b[15~")
                drain(.3)
                assert tmux("display-message", "-p", "#{window_index}") == "1"
                # A reserved key must still reach a remote terminal application.
                reader = "import os,termios,tty; old=termios.tcgetattr(0); tty.setraw(0); key=os.read(0,3); termios.tcsetattr(0,termios.TCSADRAIN,old); print('FORWARDED_'+key.hex())"
                tmux("send-keys", "-t", "essh:1", "python3 -c " + shlex.quote(reader), "Enter")
                drain(.5)
                os.write(master, b"\x01v\x1bOP")
                wait_for(lambda: "FORWARDED_1b4f50" in view("essh:1"), "reserved F1 forwarding")
                transcript.clear()
                os.write(master,b"\x1bOQ")
                wait_for(lambda: tmux("show-options","-gv","@essh_side_mode")=="1" and "RX" in view("essh:1.1") and len(tmux("list-panes","-t","essh:1").splitlines())==2,"F2 terminal to strip")
                transcript.clear()
                os.write(master,b'\x01t')
                wait_for(lambda: 'Themes ·' in transcript.decode(errors='replace'), 'Ctrl+A t picker from shell')
                os.write(master,b'\x1b')
                drain(.3)
                original_pid = tmux('display-message', '-p', '-t', 'essh:1.0', '#{pane_pid}')
                alternate_theme = 'light' if test_theme == 'terminal' else 'terminal'
                subprocess.run([str(BIN), '--theme', alternate_theme, 'tmux', 'refresh'], env=env, check=True)
                wait_for(lambda: alternate_theme in tmux('display-message', '-p', '-t', 'essh:1.1', '#{pane_start_command}'), 'theme refresh reaches side view')
                assert tmux('display-message', '-p', '-t', 'essh:1.0', '#{pane_pid}') == original_pid
                assert tmux('show-window-options', '-gv', 'window-style') == 'default'
                cli('refresh')
                wait_for(lambda: 'RX' in view('essh:1.1'), 'health view after theme refresh')
                os.write(master,b"\x1bOQ")
                wait_for(lambda: tmux("show-options","-gv","@essh_side_mode")=="2" and "CPU" in view("essh:1.1"),"F2 strip to half")
                transcript.clear()
                os.write(master,b"\x1bOQ")
                wait_for(lambda: "Health │" in transcript.decode(errors="replace"),"F2 half to full")
                health_pid = tmux('display-message', '-p', '-t', 'essh:1.0', '#{pane_pid}')
                for chosen_theme, steps in [('netwatch', 1), (test_theme, 2 if test_theme == 'light' else 0)]:
                    transcript.clear()
                    os.write(master,b't')
                    wait_for(lambda: 'Themes ·' in transcript.decode(errors='replace'), 'theme picker inside full health')
                    os.write(master,b'\x1b[H' + b'\x1b[B'*steps + b'\r')
                    wait_for(lambda: tomllib.loads((root / '.essh/config.toml').read_text()).get('theme') == chosen_theme, 'full health applies and saves theme')
                    wait_for(lambda: 'Health │' in transcript.decode(errors='replace'), 'theme apply returns to full health')
                    assert tmux('display-message', '-p', '-t', 'essh:1.0', '#{pane_pid}') == health_pid

                os.write(master,b"\x1bOQ")
                wait_for(lambda: tmux("show-options","-gv","@essh_side_mode")=="0" and len(tmux("list-panes","-t","essh:1").splitlines())==1,"F2 full to terminal")
                drain(.3)
                transcript.clear()
                os.write(master, b"\x01h")
                health_file = root / ".essh/tmux-v3" / (first + ".health")
                wait_for(lambda: health_file.exists() and "Health" in transcript.decode(errors="replace"), "direct full dashboard")
                wait_for(lambda: bool(json.loads(health_file.read_text())["details"]["cores"]), "dashboard per-core metrics")
                data=json.loads(health_file.read_text())
                assert data["details"]["mem_total"] > 0
                assert data["details"]["processes"]
                assert data["details"]["mem_cached"] is not None
                transcript.clear()
                os.write(master,b"\x1b[C")
                wait_for(lambda: "offline" in transcript.decode(errors="replace") and "not live" in transcript.decode(errors="replace"), "browse host without a session")
                os.write(master,b"\x1b[D")
                drain(.5)
                os.write(master, b"\x1bOP")
                wait_for(lambda: "GETTING AROUND" in transcript.decode(errors="replace"), "F1 contextual help")
                os.write(master, b"\x1bOP")
                drain(.3)
                os.write(master, b"b")
                wait_for(lambda: "RX" in view("essh:1.1") and "cpu" in view("essh:1.1"), "metrics strip")
                assert "RX/s" in view("essh:1.1")
                wait_for(lambda: not (root / ".essh/tmux-v3" / (first + ".health-worker")).exists(), "detailed collector stops when dashboard closes")
                assert "↓" in cli("status", "@1")
                os.write(master, b"\x01m")
                wait_for(lambda: "CPU" in view("essh:1.1") and "Processes" in view("essh:1.1"), "half-width health mode")
                assert tmux("display-message","-p","-t","essh:1.1","#{pane_width}") == "60"
                os.write(master, b"\x01m")
                wait_for(lambda: len(tmux("list-panes", "-t", "essh:1", "-F", "#{pane_id}").splitlines()) == 1, "third toggle hides side health")
                cli("open", "fixture", "--new")
                wait_for(lambda: state(2) == "connected", "second independent session")
                os.write(master,b"\x01m")
                wait_for(lambda: len(tmux("list-panes","-t","essh:2").splitlines())==2,"strip on second session")
                os.write(master,b"\x1b[15~")
                wait_for(lambda: len(tmux("list-panes","-t","essh:1").splitlines())==2,"strip mode follows session switch")
                os.write(master,b"\x01m")
                wait_for(lambda: "Processes" in view("essh:1.1"),"half mode after switch")
                os.write(master,b"\x01m")
                wait_for(lambda: len(tmux("list-panes","-t","essh:1").splitlines())==1,"hide half view")
                tmux("select-window","-t","essh:2")
                wait_for(lambda: len(tmux("list-panes","-t","essh:2").splitlines())==1,"off mode follows session switch")
                second = tmux("show-window-options", "-v", "-t", "essh:2", "@essh_id")
                assert first != second
                os.write(master, b"\x1b[15~")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="1", "F5 previous session")
                os.write(master, b"\x1b[17~")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="2", "F6 next session")
                transcript.clear()
                os.write(master, b"\x01h")
                wait_for(lambda: (root / ".essh/tmux-v3" / (second + ".health")).exists(), "second-host dashboard")
                os.write(master, b"\x1b[15~")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="1", "F5 within dashboard")
                os.write(master, b"\x1bOS")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="0", "F4 main from dashboard")
                drain(.5)
                transcript.clear()
                # Click the F3 Sessions control in the main view.
                os.write(master, b"\x1b[<0;24;1M\x1b[<0;24;1m")
                wait_for(lambda: "Sessions │ Enter" in transcript.decode(errors="replace"), "mouse sessions control")
                os.write(master, b"2")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="2", "mouse-opened switcher jump")
                assert len(tmux("list-sessions", remote=True).splitlines()) == 2
                drain(.5)
                # Real tmux status-bar mouse ranges, outside the application view.
                os.write(master,b"\x1b[<0;38;32M\x1b[<0;38;32m")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="0", "click Main status control")
                drain(.4)
                os.write(master,b"\x1b[<0;41;31M\x1b[<0;41;31m")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="2", "click session status tab")
                drain(.4)
                transcript.clear()
                os.write(master,b"\x1b[<0;10;31M\x1b[<0;10;31m")
                wait_for(lambda: "Health" in transcript.decode(errors="replace"), "click health status chip")
                os.write(master,b"\x1b")
                drain(.4)
                # The shell variable proves we reattach the old shell, rather than
                # silently substituting a freshly-created terminal after a drop.
                subprocess.run([str(wrapper), "-S", f"/tmp/essh-{os.getuid()}/{first}", "-O", "exit", "127.0.0.1"], env=env, capture_output=True, check=True)
                wait_for(lambda: "disconnected" in state(1), "disconnected state")
                assert "unknown" in state(1)
                tmux("select-window", "-t", "essh:1")
                tmux("send-keys", "-t", "essh:1", "r")
                wait_for(lambda: state(1) == "connected", "r reattaches after drop")
                tmux("send-keys", "-t", "essh:1", "echo RECONNECTED_$ESSH_PERSIST_MARKER", "Enter")
                wait_for(lambda: "RECONNECTED_works" in view("essh:1"), "persistent shell variable")
                # Exercise the actual popup key binding, not a direct CLI helper.
                os.write(master, b"\x1bOR")
                drain(0.5)
                os.write(master, b"2")
                wait_for(lambda: tmux("display-message", "-p", "#{window_index}") == "2", "F3 switcher jump")
                os.write(master, b"\x1bOS")
                wait_for(lambda: tmux("display-message", "-p", "#{window_index}") == "0", "back to hosts")
                fcntl.ioctl(master, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 60, 0, 0))
                wait_for(lambda: "USER@ADDRESS" not in view("essh:0"), "narrow host layout")
                assert "fixture" in view("essh:0")
                transcript.clear()
                os.write(master,b"\x1bOQ")
                wait_for(lambda: "Health │" in transcript.decode(errors="replace"),"narrow F2 skips side modes")
                assert len(tmux("list-panes","-t","essh:2").splitlines())==1
                os.write(master,b"\x1bOQ")
                drain(.4)
                os.write(master,b"\x1bOS")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="0","main after narrow cycle")
                os.write(master, b"q")
                wait_for(lambda: client.poll() is not None, "quit detaches")
                assert len(tmux("list-sessions", remote=True).splitlines()) == 2
                tmux("kill-server")
                # Restore IDs after loss of the local controller; no auto-connect.
                client = launch()
                wait_for(lambda: len(tmux("list-windows", "-F", "#{@essh_id}", check=False).split()) == 2, "saved session restoration")
                wait_for(lambda: tmux("display-message", "-p", "#{session_attached}") == "1", "restored client attached")
                restored = tmux("list-windows", "-F", "#{@essh_id}").split()
                assert tmux("show-window-options","-v","-t","essh:1","@essh_id")==first
                assert tmux("show-window-options","-v","-t","essh:2","@essh_id")==second
                assert sorted(restored) == sorted([first, second])
                wait_for(lambda: "unknown" in state(1) and "unknown" in state(2), "restored entries stay unverified")
                # Explicit termination must delete recovery metadata before the popup
                # disappears with its parent window.
                tmux("select-window", "-t", "essh:1")
                killed_id = tmux("show-window-options", "-v", "-t", "essh:1", "@essh_id")
                os.write(master, b"\x01x")
                drain(0.5)
                os.write(master,b"n")
                drain(.3)
                assert len(tmux("list-sessions",remote=True).splitlines())==2
                os.write(master,b"\x01x")
                drain(.4)
                os.write(master, b"y")
                wait_for(lambda: len(tmux("list-windows", "-F", "#{@essh_id}").split()) == 1, "confirmed kill")
                assert not (root / ".essh/tmux-v3" / (killed_id + ".json")).exists()
                assert len(tmux("list-sessions", remote=True).splitlines()) == 1
                remaining = tmux("list-windows", "-F", "#{window_id} #{@essh_id}").splitlines()
                remaining = next(line.split() for line in remaining if len(line.split()) == 2)
                target, retained_id = remaining
                subprocess.run([str(wrapper), "-S", f"/tmp/essh-{os.getuid()}/{retained_id}", "-O", "exit", "127.0.0.1"], env=env, capture_output=True)
                server.terminate()
                server.wait(timeout=5)
                tmux("select-window", "-t", target)
                os.write(master, b"\x01x")
                drain(0.5)
                os.write(master, b"y")
                drain(2)
                assert (root / ".essh/tmux-v3" / (retained_id + ".json")).exists()
                assert len(tmux("list-windows", "-F", "#{@essh_id}").split()) == 1
                assert len(tmux("list-sessions", remote=True).splitlines()) == 1
                os.write(master, b"\n")
                print("PASS: forms, filters, host health browsing, strip/half/off persistence, F1–F6, mouse navigation, real metrics, reconnect, resize, restore, kill confirmation and failed-kill retention")
            finally:
                tmux("kill-server", check=False)
                tmux("kill-server", remote=True, check=False)
                # Stop fixture masters without touching the user's other sockets.
                for record in (root / ".essh/tmux-v3").glob("*.json"):
                    session_id = json.loads(record.read_text())["id"]
                    subprocess.run([str(wrapper), "-S", f"/tmp/essh-{os.getuid()}/{session_id}", "-O", "exit", "127.0.0.1"], env=env, capture_output=True)
                if client and client.poll() is None:
                    client.terminate()
                server.terminate()
                server.wait(timeout=5)
                os.close(master)
                os.close(slave)


if __name__ == "__main__":
    main()
