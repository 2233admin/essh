#!/usr/bin/env python3
"""Record and verify the real v3 interface in an isolated SSH fixture.

Based on the live v3 regression test. Requires Unix, tmux 3.3+, sshd, ssh and ssh-keygen.

Uses a loopback-only sshd, ephemeral keys, private known_hosts and isolated local
and remote tmux sockets. Does not alter the user's configuration or services.
Run after cargo build --release: python3 scripts/record-tmux-demo.py
"""
import codecs
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

BIN = Path(os.environ.get("ESSH_BIN", Path(__file__).resolve().parents[1] / "target/release/essh"))


def main():
    for tool in ("tmux", "sshd", "ssh", "ssh-keygen"):
        if not shutil.which(tool):
            raise SystemExit(f"Required fixture tool missing: {tool}")
    if not BIN.exists():
        raise SystemExit("Build essh first with cargo build")
    with tempfile.TemporaryDirectory(prefix="essh-v3-demo-") as temp:
        root = Path(temp)
        for directory in (".essh", "local", "remote", "bin"):
            (root / directory).mkdir()
        env = dict(os.environ, HOME=temp, TMUX_TMPDIR=str(root / "local"), TERM="xterm-256color")
        env.pop("TMUX", None)
        env.pop("NO_COLOR", None)
        env["COLORTERM"] = "truecolor"
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
        output = Path(os.environ.get('ESSH_DEMO_OUT', 'artifacts/demo')).resolve()
        output.mkdir(parents=True, exist_ok=True)
        master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 46, 192, 0, 0))
        events, chapters, checks, speedups = [], [], [], []
        decoder = codecs.getincrementaldecoder("utf-8")(errors="strict")
        transcript = bytearray()
        started = time.monotonic()
        client = None

        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        def drain(seconds=.1):
            deadline = time.monotonic() + seconds
            while time.monotonic() < deadline:
                if select.select([master], [], [], .04)[0]:
                    try:
                        raw = os.read(master, 65536)
                    except OSError:
                        break
                    if not raw:
                        break
                    transcript.extend(raw)
                    events.append([round(time.monotonic()-started, 3), 'o', decoder.decode(raw)])

        def tmux(*args, remote=False, check=True):
            result = subprocess.run(['tmux', '-L', 'essh' if remote else 'essh-v3', *args],
                env=remote_env if remote else env, capture_output=True, text=True)
            if check:
                assert result.returncode == 0, result.stderr
            return result.stdout.strip()

        def cli(*args):
            result = subprocess.run([str(BIN), 'tmux', *args], env=env, capture_output=True, text=True)
            assert result.returncode == 0, result.stderr

        def view(target='essh:1'):
            return tmux('capture-pane', '-p', '-t', target, check=False)

        def window():
            return tmux('display-message', '-p', '#{window_index}')

        def state(index):
            return tmux('show-window-options', '-v', '-t', f'essh:{index}', '@essh_state', check=False)

        def wait_for(predicate, name, seconds=20):
            wait_start = time.monotonic()-started
            deadline = time.monotonic()+seconds
            while time.monotonic() < deadline:
                drain()
                if predicate():
                    wait_end = time.monotonic()-started
                    checks.append({'check': name, 'passed': True, 'time': round(wait_end, 3)})
                    if wait_end-wait_start > 1 and ('connected' in name.lower() or name == 'r reconnects'):
                        speedups.append({'start':wait_start,'end':wait_end,'duration':.6,'reason':name})
                    return
            raise AssertionError(f'{name}\n{view()}\n{transcript[-3000:]!r}')

        def chapter(title, keys):
            chapters.append({'time': round(time.monotonic()-started, 3), 'title': title, 'keys': keys})
            print(title, flush=True)

        def checkpoint(*needles):
            chapters[-1]['sample'] = round(time.monotonic()-started-.4, 3)
            chapters[-1]['expect'] = list(needles)

        def command(text, expected, hold=1.2):
            press(text.encode())
            drain(.3)
            press(b'\r')
            wait_for(lambda: expected in view(f'essh:{window()}.0'), 'Admin command: '+text)
            drain(hold)

        def press(keys):
            transcript.clear()
            os.write(master, keys)

        def seen(text):
            return text in transcript.decode(errors='replace')

        server = subprocess.Popen([shutil.which('sshd'), '-D', '-e', '-f', str(root/'sshd_config')],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            time.sleep(.3)
            assert server.poll() is None, 'fixture sshd failed'
            chapter('Main menu · configured hosts', 'F4 Main menu  / Filter  Enter Connect')
            client = subprocess.Popen([str(BIN)], stdin=slave, stdout=slave, stderr=slave, env=env, preexec_fn=controlling_terminal)
            wait_for(lambda: 'fixture' in view('essh:0'), 'Hosts visible')
            drain(.6)
            chapter('Find a host and connect', '/ fixture → Enter')
            press(b'/fixture')
            drain(.6)
            press(b'\r')
            wait_for(lambda: state(1)=='connected', 'Real SSH connected')
            press(b'export ESSH_PERSIST_MARKER=works\r')
            drain(.5)
            chapter('Storage triage · filesystems and block devices', 'df -hT  ·  lsblk -o NAME,SIZE,TYPE,MOUNTPOINTS')
            command('clear; df -hT / /home /tmp; lsblk -o NAME,SIZE,TYPE,MOUNTPOINTS | head -12', 'MOUNTPOINTS', 1.5)
            checkpoint('Filesystem', 'MOUNTPOINTS')
            first = tmux('show-window-options', '-v', '-t', 'essh:1', '@essh_id')
            chapter('Side health · thin meters beside your terminal', 'F2 · terminal → strip')
            press(b'\x1bOQ')
            wait_for(lambda: tmux('show-options','-gv','@essh_side_mode')=='1' and 'cpu' in view('essh:1.1'), 'F2 side strip')
            command('ps -eo pid,comm,%cpu,%mem --sort=-%cpu | head -9', '%CPU', 2)
            checkpoint('cpu', '%CPU')
            chapter('Half health · terminal and dashboard together', 'F2 · strip → half')
            press(b'\x1bOQ')
            wait_for(lambda: tmux('show-options','-gv','@essh_side_mode')=='2' and 'Processes' in view('essh:1.1'), 'F2 half dashboard')
            command('vmstat 1 3', 'swpd', 3)
            checkpoint('Processes', 'swpd')
            chapter('Full health · network, CPU, memory, disks and processes', 'F2 · half → full')
            press(b'\x1bOQ')
            wait_for(lambda: seen('Health │'), 'F2 full dashboard')
            health_file=root/'.essh/tmux-v3'/f'{first}.health'
            wait_for(lambda: health_file.exists() and bool(json.loads(health_file.read_text()).get('details',{}).get('cores')), 'Live per-core telemetry')
            data=json.loads(health_file.read_text())
            assert data['details']['mem_total']>0 and data['details']['processes']
            checks.append({'check':'Real memory and process telemetry', 'passed':True})
            drain(4)
            checkpoint('Storage', 'Processes')
            chapter('Browse hosts · disconnected hosts are clearly marked', '→ Next host  ← Previous host')
            press(b'\x1b[C')
            wait_for(lambda: seen('offline') and seen('not live'), 'Offline host has no fabricated live data')
            drain(1.2)
            checkpoint('not live')
            press(b'\x1b[D')
            drain(.3)
            chapter('Return to your terminal', 'F2 · full → terminal')
            press(b'\x1bOQ')
            wait_for(lambda: tmux('show-options','-gv','@essh_side_mode')=='0' and len(tmux('list-panes','-t','essh:1').splitlines())==1,'F2 completes cycle without duplicate panes')
            drain(.6)
            chapter('Open another session', 'F3 Sessions  n New session  Enter Connect')
            press(b'\x1bOR')
            wait_for(lambda: seen('Sessions │ Enter'), 'F3 session picker')
            drain(.6)
            press(b'n')
            drain(.3)
            press(b'\r')
            wait_for(lambda: state(2)=='connected', 'Second independent session connected')
            second=tmux('show-window-options','-v','-t','essh:2','@essh_id')
            assert first!=second
            chapter('Network triage · listening sockets and interface state', 'ss -lntu  ·  ip -br address')
            command('clear; ss -lntu | head -12; ip -br address', 'Netid', 2)
            checkpoint('Netid', 'State')
            chapter('Switch sessions with keys', 'F5 Previous  F6 Next')
            press(b'\x1b[15~')
            wait_for(lambda: window()=='1','F5 previous session')
            drain(.6)
            press(b'\x1b[17~')
            wait_for(lambda: window()=='2','F6 next session')
            drain(.6)
            chapter('Return to Main · sessions keep running', 'F4 Main menu')
            press(b'\x1bOS')
            wait_for(lambda: window()=='0','F4 Main menu')
            assert len(tmux('list-sessions',remote=True).splitlines())==2
            drain(.6)
            chapter('Mouse navigation · click Sessions, then choose a session', 'Click Sessions  1 Select first session')
            press(b'\x1b[<0;24;1M\x1b[<0;24;1m')
            wait_for(lambda: seen('Sessions │ Enter'),'Mouse opens Sessions')
            drain(.6)
            press(b'1')
            wait_for(lambda: window()=='1','Number key selects session')
            drain(.6)
            chapter('Built-in navigation help', 'F1 Help  F1 Close')
            press(b'\x1bOP')
            wait_for(lambda: seen('GETTING AROUND'),'F1 documented navigation help')
            drain(3)
            checkpoint('GETTING AROUND')
            press(b'\x1bOP')
            drain(.5)
            chapter('Connection drops · reconnect to the same remote shell', 'Simulated connection drop → r Reconnect')
            subprocess.run([str(wrapper),'-S',f'/tmp/essh-{os.getuid()}/{first}','-O','exit','127.0.0.1'],env=env,capture_output=True,check=True)
            wait_for(lambda:'disconnected' in state(1),'Connection failure shown')
            drain(1.2)
            press(b'r')
            wait_for(lambda:state(1)=='connected','r reconnects')
            press(b'echo RECONNECTED_$ESSH_PERSIST_MARKER\r')
            wait_for(lambda:'RECONNECTED_works' in view(),'Remote shell state survives reconnect')
            drain(1.5)
            checkpoint('RECONNECTED_works')
            chapter('ESSH · all four health views, sessions and recovery', 'F1 Help  F2 Health  F3 Sessions  F4 Main  F5/F6 Switch')
            press(b'\x01h')
            wait_for(lambda:seen('Health │'),'Direct full dashboard shortcut')
            drain(2)
            checkpoint('Health │')
        finally:
            drain(.2)
            duration=round(time.monotonic()-started,3)
            (output/'essh-v3-demo.cast').write_text(json.dumps({'version':2,'width':192,'height':46,'timestamp':int(time.time()),'duration':duration,'env':{'TERM':'xterm-256color'}})+'\n'+'\n'.join(json.dumps(e) for e in events)+'\n')
            (output/'speedups.json').write_text(json.dumps(speedups,indent=2)+'\n')
            (output/'chapters.json').write_text(json.dumps(chapters,indent=2)+'\n')
            (output/'verification.json').write_text(json.dumps({'binary':str(BIN),'duration':duration,'checks':checks},indent=2)+'\n')
            tmux('kill-server',check=False)
            tmux('kill-server',remote=True,check=False)
            for path in (root/'.essh/tmux-v3').glob('*.json'):
                subprocess.run([str(wrapper),'-S',f'/tmp/essh-{os.getuid()}/{path.stem}','-O','exit','127.0.0.1'],env=env,capture_output=True)
            if client:
                client.wait(timeout=5)
            server.terminate()
            server.wait(timeout=5)
            os.close(master)
            os.close(slave)
        print(f'PASS: {len(checks)} recorded checks; {duration}s; {output}',flush=True)


if __name__ == '__main__':
    main()
