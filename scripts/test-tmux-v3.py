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
""")
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 32, 120, 0, 0))
        client = None
        transcript = bytearray()

        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        def launch():
            return subprocess.Popen([str(BIN)], stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=controlling_terminal)

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
            result = subprocess.run([str(BIN), "tmux", *args], env=env, capture_output=True, text=True)
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
                wait_for(lambda: "USER@ADDRESS" in view("essh:0"), "wide host layout")
                os.write(master, b"\x1bOQ")
                wait_for(lambda: "Connect to a host" in transcript.decode(errors="replace"), "health without a session")
                os.write(master, b"\x1b")
                drain(.3)
                os.write(master, b"\x1b[15~")
                drain(.3)
                assert tmux("display-message", "-p", "#{window_index}") == "0"
                cli("open", "fixture")
                wait_for(lambda: state(1) == "connected", "SSH connection")
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
                os.write(master, b"\x1bOQ")
                health_file = root / ".essh/tmux-v3" / (first + ".health")
                wait_for(lambda: health_file.exists() and "Health" in transcript.decode(errors="replace"), "F2 full dashboard")
                wait_for(lambda: bool(json.loads(health_file.read_text())["details"]["cores"]), "dashboard per-core metrics")
                data=json.loads(health_file.read_text())
                assert data["details"]["mem_total"] > 0
                assert data["details"]["processes"]
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
                wait_for(lambda: len(tmux("list-panes", "-t", "essh:1", "-F", "#{pane_id}").splitlines()) == 1, "compatibility shortcut hides strip")
                cli("open", "fixture", "--new")
                wait_for(lambda: state(2) == "connected", "second independent session")
                second = tmux("show-window-options", "-v", "-t", "essh:2", "@essh_id")
                assert first != second
                os.write(master, b"\x1b[15~")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="1", "F5 previous session")
                os.write(master, b"\x1b[17~")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="2", "F6 next session")
                transcript.clear()
                os.write(master, b"\x1bOQ")
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
                os.write(master,b"\x1b[<0;34;31M\x1b[<0;34;31m")
                wait_for(lambda: tmux("display-message","-p","#{window_index}")=="2", "click session status tab")
                # The shell variable proves we reattach the old shell, rather than
                # silently substituting a freshly-created terminal after a drop.
                subprocess.run([str(wrapper), "-S", f"/tmp/essh-{os.getuid()}/{first}", "-O", "exit", "127.0.0.1"], env=env, capture_output=True, check=True)
                wait_for(lambda: "disconnected" in state(1), "disconnected state")
                assert "unknown" in state(1)
                tmux("select-window", "-t", "essh:1")
                tmux("send-keys", "-t", "essh:1", "Enter")
                wait_for(lambda: state(1) == "connected", "reattach after drop")
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
                os.write(master, b"yes\n")
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
                os.write(master, b"yes\n")
                drain(2)
                assert (root / ".essh/tmux-v3" / (retained_id + ".json")).exists()
                assert len(tmux("list-windows", "-F", "#{@essh_id}").split()) == 1
                assert len(tmux("list-sessions", remote=True).splitlines()) == 1
                os.write(master, b"\n")
                print("PASS: F1–F6 navigation, full dashboard, per-core/process metrics, mouse navigation, strip compatibility, reconnect, resize, restore, kill and failed-kill retention")
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
