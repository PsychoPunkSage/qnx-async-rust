#!/bin/sh
# Run the echo benchmark matrix on a QNX target over SSH: the same server built
# on mio's poll() selector and on the ionotify selector, at several connection
# counts. Selectors alternate within each cell so drift hits both equally.
#
# Needs the QNX SDP environment (qnxsdp-env.sh) for the build, and root on the
# target to raise the fd and registered-event limits.
#
# Tunables (environment): QNX_SSH_HOST, RUNS, TOTALS, ACTIVES, WARMUP, MEASURE.

set -eu
cd "$(dirname "$0")"

host="${QNX_SSH_HOST:-qnx}"
runs="${RUNS:-3}"
totals="${TOTALS:-10 100 1000}"
actives="${ACTIVES:-1 8}"
warmup="${WARMUP:-2}"
measure="${MEASURE:-10}"
remote=/data/tmp/bench

cargo build --release
RUSTFLAGS="--cfg mio_unsupported_force_poll_poll" CARGO_TARGET_DIR=target/poll \
    cargo build --release

ssh "$host" mkdir -p "$remote"
scp -q target/x86_64-pc-qnx/release/client "$host:$remote/client"
scp -q target/x86_64-pc-qnx/release/server "$host:$remote/server-ionotify"
scp -q target/poll/x86_64-pc-qnx/release/server "$host:$remote/server-poll"

stamp=$(date +%Y%m%d-%H%M%S)
mkdir -p results
csv="results/$stamp.csv"
setup="results/$stamp-setup.txt"

{
    echo "date: $(date -u)"
    echo "runs per cell: $runs, warmup: ${warmup}s, measure: ${measure}s, message: 64 B"
    echo "mio: $(git -C ../../mio rev-parse --short HEAD 2>/dev/null || echo unknown)$(git -C ../../mio diff --quiet 2>/dev/null || echo ' (dirty)')"
    echo "rustc: $(rustc --version)"
    echo "--- target"
    ssh "$host" 'uname -a; pidin info | head -8'
} > "$setup"

echo "run,label,total,active,requests,rps,p50_us,p99_us,p999_us,server_cpu_ms,iosock_cpu_ms,cpu_us_per_req" > "$csv"
port=7200
for run in $(seq "$runs"); do
    for total in $totals; do
        for active in $actives; do
            for sel in poll ionotify; do
                port=$((port + 1))
                row=$(ssh "$host" "cd $remote; ./server-$sel $port & pid=\$!; sleep 1; \
                    ./client 127.0.0.1:$port $total $active $warmup $measure \$pid $sel; \
                    kill \$pid; wait \$pid 2>/dev/null; true")
                echo "$run,$row" | tee -a "$csv"
            done
        done
    done
done
echo "wrote $csv and $setup"
