#!/usr/bin/env bash
# The fixture fleet from docs/redesign/SPEC.md §2, as containers.
#
#   ./scripts/fleet.sh up        build if needed, create, start, stage
#   ./scripts/fleet.sh check     report drift without changing anything
#   ./scripts/fleet.sh down      stop everything
#   ./scripts/fleet.sh recreate  destroy and rebuild from the image
#
# Why this exists. The demo asserts the spec's rules against eleven real
# containers, and the staging that makes them disagree lives in their running
# state: nginx listening, redis listening, a timezone, a hand-edited sshd
# config. A container restart keeps the filesystem and loses the processes, so
# the fleet comes back reachable and unstaged. `demo_verify` then fails on
# "web-02 is not serving :80" and blames the app for a fact about the
# laptop. Staging is part of the fixture, so it is scripted like the rest of
# it.
#
# `up` is idempotent and never destroys a container that already exists.
set -euo pipefail

cd "$(dirname "$0")/.."
IMAGE=essh-fixture:latest
KEY="${SSH_KEY:-$HOME/.ssh/id_ed25519}"

# name:address:role. SPEC §2.1, in the order the config file lists them.
HOSTS=(
  "web-01:127.0.0.11:web"
  "web-02:127.0.0.12:web"
  "web-03:127.0.0.13:web"
  "db-01:127.0.0.14:db"
  "db-02:127.0.0.15:db-drift"
  "cache-01:127.0.0.16:cache"
  "edge-01:127.0.0.17:edge"
  "build-01:127.0.0.18:build"
  "monitor-01:127.0.0.19:monitor"
  "staging-web-01:127.0.0.20:offline"
  "jump-01:127.0.0.21:jump"
)

die() { echo "$*" >&2; exit 1; }
have() { podman container exists "$1" 2>/dev/null; }
running() { [ "$(podman inspect -f '{{.State.Running}}' "$1" 2>/dev/null)" = "true" ]; }

build_image() {
  if podman image exists "$IMAGE"; then return 0; fi
  [ -f "$KEY.pub" ] || die "no public key at $KEY.pub; set SSH_KEY"
  local ctx
  ctx=$(mktemp -d)
  # Expanded now, not at RETURN: `ctx` is a local and is gone by the time the
  # trap fires, which under `set -u` turns cleanup into an error.
  # shellcheck disable=SC2064
  trap "rm -rf '$ctx'" RETURN
  cp scripts/fixture/Containerfile "$ctx/Containerfile"
  cp "$KEY.pub" "$ctx/authorized_keys"
  echo "building $IMAGE"
  podman build -t "$IMAGE" "$ctx"
}

create_missing() {
  for spec in "${HOSTS[@]}"; do
    IFS=: read -r name ip _ <<<"$spec"
    if have "$name"; then continue; fi
    echo "creating $name on $ip:2222"
    podman create --name "$name" --hostname "$name" \
      -p "$ip:2222:22" "$IMAGE" >/dev/null
  done
}

# The disagreements the fleet is built to have. Genuine differences between
# genuine hosts, deliberately created: SPEC §2 wants the demo's numbers to be
# collected, not drawn.
stage() {
  local name=$1 role=$2
  case "$role" in
    web)
      podman exec "$name" sh -c 'pgrep -x nginx >/dev/null || nginx' ;;
    cache)
      podman exec "$name" sh -c \
        'pgrep -x redis-server >/dev/null || redis-server --daemonize yes' ;;
    db-drift)
      # A comment appended to a file every other host holds identical. The
      # smallest divergence the collector can find, and the one that proves
      # it hashes contents rather than paths.
      podman exec "$name" sh -c \
        'grep -q "pinned by hand" /etc/nsswitch.conf \
         || echo "# pinned by hand 2026-08-14" >> /etc/nsswitch.conf' ;;
    edge)
      # Two facets at once: a timezone and a config file. The timezone is the
      # one that reverts on restart, which is what made this script necessary.
      podman exec "$name" sh -c \
        'ln -sf /usr/share/zoneinfo/Europe/Dublin /etc/localtime;
         echo Europe/Dublin > /etc/timezone;
         grep -q "hand-edited on the edge box" /etc/ssh/sshd_config \
         || printf "\n# hand-edited on the edge box, 2026-08-14\nClientAliveInterval 30\n" \
              >> /etc/ssh/sshd_config' ;;
    build)
      # Real load on a real host, so the health screen has something to be
      # about. SPEC §2.1 puts build-01 over the CPU threshold with a named
      # cause, and staging that by drawing it would verify nothing: the
      # attention row has to name a process the sampler actually found.
      #
      # `nice -n 19` so the laptop stays usable, and `timeout` so a fixture
      # left running does not pin a core until reboot.
      #
      # A file on disk rather than an inline `sh -c`, because the process has
      # to be findable by name: `ps` is what the sampler reads, and a row
      # reading `sh -c while :; do :; done` tells nobody what it is. It also
      # has to outlive the exec that starts it, which a backgrounded child of
      # `podman exec -d` does not.
      podman exec "$name" sh -c \
        'cat > /usr/local/bin/essh-fixture-load <<"LOADEOF"
#!/bin/sh
# Deliberate load, so the health screen has something real to be about.
#
# The deadline is checked every 200k iterations rather than every one: a
# `date` call per iteration would make this a syscall benchmark instead of a
# busy loop, and the point is to occupy a core.
#
# No `exec timeout ...`: that would replace this script and `ps` would show
# `timeout`, so the health screen would name a cause nobody can act on.
end=$(($(date +%s) + ${1:-1800}))
i=0
while :; do
  i=$((i + 1))
  if [ $((i % 200000)) -eq 0 ] && [ "$(date +%s)" -ge "$end" ]; then exit 0; fi
done
LOADEOF
         chmod +x /usr/local/bin/essh-fixture-load'
      for _ in 1 2 3; do
        podman exec -d "$name" nice -n 19 \
          /usr/local/bin/essh-fixture-load "${LOAD_SECS:-7200}"
      done ;;
    db)
      # A tool for the demo to run, not a process to leave running. The
      # sessions screen's RUNNING, CPU and LAST LINE columns need a session
      # that is doing something, and three idle shells demonstrate none of
      # them — but a fixture that starts jobs nobody asked for is a fixture
      # that lies about what the host was doing.
      #
      # One long-lived process rather than a loop of short ones: `ps aux`
      # reports %CPU as a lifetime average, so a loop respawning a
      # sub-second command reports near zero however hard it is working.
      podman exec "$name" sh -c \
        'cat > /usr/local/bin/nightly-verify <<"VERIFYEOF"
#!/usr/bin/perl
# Checksum every file under the given root, forever. Real reads, real CPU,
# one line of real output per file — which is what the sessions screen needs
# to have something true to show.
my $root = shift || "/usr";
$| = 1;
while (1) {
  open(my $find, "-|", "find", $root, "-xdev", "-type", "f") or die;
  while (my $f = <$find>) {
    chomp $f;
    open(my $h, "<", $f) or next;
    binmode $h;
    my $sum = 0;
    while (read($h, my $buf, 65536)) { $sum += unpack("%32C*", $buf) }
    close $h;
    printf "%-60s %08x\n", $f, $sum;
  }
  close $find;
}
VERIFYEOF
         chmod +x /usr/local/bin/nightly-verify' ;;
    monitor|jump)
      : ;;
    offline)
      : ;;
  esac
}

# What each role should look like once staged, as a command that exits 0.
# `check` runs these; `up` runs them after staging so a silent failure in
# `stage` is reported rather than assumed away.
verify_role() {
  local name=$1 role=$2
  case "$role" in
    web)      podman exec "$name" sh -c 'ss -ltn | grep -q ":80 "' ;;
    cache)    podman exec "$name" sh -c 'ss -ltn | grep -q ":6379 "' ;;
    db-drift) podman exec "$name" sh -c 'grep -q "pinned by hand" /etc/nsswitch.conf' ;;
    edge)     podman exec "$name" sh -c \
                '[ "$(cat /etc/timezone)" = Europe/Dublin ] \
                 && grep -q "hand-edited on the edge box" /etc/ssh/sshd_config' ;;
    # `pgrep -f` would match the `sh -c` running the check, so this counts
    # what `ps` actually shows — which is also what the sampler reads, and
    # what the health screen will name as the cause.
    build)    [ "$(podman exec "$name" sh -c \
                 'ps -eo args | grep -c "[e]ssh-fixture-load"')" -ge 1 ] ;;
    db)       podman exec "$name" test -x /usr/local/bin/nightly-verify ;;
    *)        true ;;
  esac
}

reachable() {
  ssh -p 2222 -o BatchMode=yes -o StrictHostKeyChecking=no \
      -o UserKnownHostsFile=/dev/null -o ConnectTimeout=4 \
      -i "$KEY" "matt@$1" true 2>/dev/null
}

cmd_up() {
  command -v podman >/dev/null || die "need podman"
  build_image
  create_missing
  for spec in "${HOSTS[@]}"; do
    IFS=: read -r name ip role <<<"$spec"
    if [ "$role" = offline ]; then
      # SPEC §9: the offline host in the demo is a container that is actually
      # down. Staging an offline state by drawing it would test nothing.
      if running "$name"; then podman stop -t 2 "$name" >/dev/null; fi
      echo "  $name  stopped (offline by design)"
      continue
    fi
    if ! running "$name"; then podman start "$name" >/dev/null; fi
  done

  # sshd needs a moment before the staging execs land.
  for spec in "${HOSTS[@]}"; do
    IFS=: read -r name ip role <<<"$spec"
    if [ "$role" = offline ]; then continue; fi
    for _ in $(seq 40); do if reachable "$ip"; then break; fi; sleep 0.25; done
  done

  for spec in "${HOSTS[@]}"; do
    IFS=: read -r name ip role <<<"$spec"
    if [ "$role" = offline ]; then continue; fi
    stage "$name" "$role" >/dev/null 2>&1 || true
  done

  # `podman exec -d` returns before the process it started is in `ps`, and
  # nginx and redis take a moment to bind. Checking immediately reports drift
  # that is only a race, which is worse than no check: it teaches you to
  # ignore the one thing that tells you the fixture is wrong.
  #
  # `fleet_ok` and not `cmd_check`: the latter reports and then `die`s, and a
  # function that exits the script cannot be a loop condition. Used as one, the
  # first miss killed the retry it was the retry for — which is how a demo
  # render came to fail on staging that would have been there a second later.
  for _ in $(seq 20); do
    if fleet_ok; then break; fi
    sleep 0.25
  done
  cmd_check
}

# The same checks, quietly, as a predicate. `cmd_check` reports and dies; this
# only answers.
fleet_ok() {
  for spec in "${HOSTS[@]}"; do
    IFS=: read -r name ip role <<<"$spec"
    if [ "$role" = offline ]; then
      if running "$name"; then return 1; fi
      continue
    fi
    if ! reachable "$ip"; then return 1; fi
    if ! verify_role "$name" "$role" 2>/dev/null; then return 1; fi
  done
  return 0
}

cmd_check() {
  local bad=0
  for spec in "${HOSTS[@]}"; do
    IFS=: read -r name ip role <<<"$spec"
    if [ "$role" = offline ]; then
      if running "$name"; then
        echo "  $name  UP, and the fixture wants it down"; bad=1
      else
        echo "  $name  offline (by design)"
      fi
      continue
    fi
    if ! reachable "$ip"; then
      echo "  $name  UNREACHABLE on $ip:2222"; bad=1; continue
    fi
    if verify_role "$name" "$role" 2>/dev/null; then
      echo "  $name  ok"
    else
      echo "  $name  reachable, but role '$role' is not staged"; bad=1
    fi
  done
  [ "$bad" = 0 ] || die "fleet is not the fixture SPEC §2 describes; run: ./scripts/fleet.sh up"
  echo "fleet: ${#HOSTS[@]} hosts, staged"
}

cmd_down() {
  for spec in "${HOSTS[@]}"; do
    IFS=: read -r name _ _ <<<"$spec"
    if running "$name"; then podman stop -t 2 "$name" >/dev/null; fi
  done
  echo "fleet: stopped"
}

cmd_recreate() {
  for spec in "${HOSTS[@]}"; do
    IFS=: read -r name _ _ <<<"$spec"
    if have "$name"; then podman rm -f "$name" >/dev/null; fi
  done
  if podman image exists "$IMAGE"; then podman rmi -f "$IMAGE" >/dev/null; fi
  cmd_up
}

case "${1:-up}" in
  up)       cmd_up ;;
  check)    cmd_check ;;
  down)     cmd_down ;;
  recreate) cmd_recreate ;;
  *)        die "usage: $0 [up|check|down|recreate]" ;;
esac
