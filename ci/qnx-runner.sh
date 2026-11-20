#!/bin/sh
# Cargo runner: copy a binary to the QNX target, run it there, then remove it.
# Usage (set by cargo): qnx-runner.sh <binary> [args...]

set -e
host="${QNX_SSH_HOST:-qnx}"
bin="$1"
shift

# rustdoc names every doctest binary `rust_out` and runs them in parallel, so the
# remote name gets the runner's PID to keep concurrent runs from overwriting
# each other.
remote="/data/tmp/$(basename "$bin").$$"

ssh "$host" mkdir -p /data/tmp
scp -q "$bin" "$host:$remote"

# /tmp on mkqnximage targets is a symlink to /data/var/tmp; QNX reports Unix
# socket paths with the symlink resolved, so point TMPDIR at the real path.
status=0
ssh "$host" "TMPDIR=/data/var/tmp" "$remote" "$@" || status=$?
ssh "$host" rm -f "$remote"
exit $status
