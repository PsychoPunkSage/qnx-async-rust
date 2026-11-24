//! Load generator for the echo benchmark. Opens `total` connections to the
//! server; `active` of them run a closed-loop ping-pong of a small message,
//! the rest stay idle. Idle connections are what a `poll()`-based selector
//! pays for on every wake, so `total` is the variable that matters.
//!
//! Uses std only, so the client is the same whichever selector the server was
//! built with.
//!
//! Usage: `client <addr> <total> <active> <warmup_s> <measure_s> <server_pid> <label>`
//!
//! Prints one CSV row:
//! `label,total,active,requests,rps,p50_us,p99_us,p999_us,server_cpu_ms,iosock_cpu_ms,cpu_us_per_req`

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Bytes per request and per reply.
const MESSAGE: usize = 64;
/// Enough for 1000+ connections plus stdio.
const MAX_FDS: u64 = 4096;

const WARMUP: u8 = 0;
const MEASURE: u8 = 1;
const STOP: u8 = 2;

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 8 {
        eprintln!(
            "usage: client <addr> <total> <active> <warmup_s> <measure_s> <server_pid> <label>"
        );
        std::process::exit(2);
    }
    let addr: SocketAddr = args[1].parse().expect("addr");
    let total: usize = args[2].parse().expect("total");
    let active: usize = args[3].parse().expect("active");
    let warmup = Duration::from_secs(args[4].parse().expect("warmup_s"));
    let measure = Duration::from_secs(args[5].parse().expect("measure_s"));
    let server_pid: libc::pid_t = args[6].parse().expect("server_pid");
    let label = &args[7];
    assert!(active >= 1 && active <= total, "need 1 <= active <= total");

    bench::raise_fd_limit(MAX_FDS)?;
    let iosock_pid = bench::find_pid("io-sock")?;

    // Idle connections: opened, then never used.
    let idle = (0..total - active)
        .map(|_| TcpStream::connect(addr))
        .collect::<io::Result<Vec<_>>>()?;

    let phase = Arc::new(AtomicU8::new(WARMUP));
    let workers = (0..active)
        .map(|_| {
            let stream = TcpStream::connect(addr)?;
            stream.set_nodelay(true)?;
            let phase = phase.clone();
            Ok(thread::spawn(move || ping_pong(stream, &phase)))
        })
        .collect::<io::Result<Vec<_>>>()?;

    thread::sleep(warmup);
    let server_before = bench::cpu_time_ns(server_pid)?;
    let iosock_before = bench::cpu_time_ns(iosock_pid)?;
    let start = Instant::now();
    phase.store(MEASURE, Ordering::Relaxed);

    thread::sleep(measure);
    phase.store(STOP, Ordering::Relaxed);
    let elapsed = start.elapsed();
    let server_ns = bench::cpu_time_ns(server_pid)? - server_before;
    let iosock_ns = bench::cpu_time_ns(iosock_pid)? - iosock_before;

    let mut rtts: Vec<u64> = Vec::new();
    for worker in workers {
        rtts.extend(worker.join().expect("worker")?);
    }
    drop(idle);
    rtts.sort_unstable();

    let requests = rtts.len() as u64;
    let pct = |p: f64| -> f64 {
        if rtts.is_empty() {
            return f64::NAN;
        }
        let i = ((rtts.len() as f64 * p) as usize).min(rtts.len() - 1);
        rtts[i] as f64 / 1000.0
    };
    let cpu_per_req = if requests == 0 {
        f64::NAN
    } else {
        (server_ns + iosock_ns) as f64 / requests as f64 / 1000.0
    };
    println!(
        "{label},{total},{active},{requests},{:.0},{:.1},{:.1},{:.1},{:.1},{:.1},{:.2}",
        requests as f64 / elapsed.as_secs_f64(),
        pct(0.50),
        pct(0.99),
        pct(0.999),
        server_ns as f64 / 1e6,
        iosock_ns as f64 / 1e6,
        cpu_per_req,
    );
    Ok(())
}

/// Send a message, wait for the whole echo, repeat. Round-trip times are kept
/// only during the measurement phase.
fn ping_pong(mut stream: TcpStream, phase: &AtomicU8) -> io::Result<Vec<u64>> {
    let request = [0x5a; MESSAGE];
    let mut reply = [0; MESSAGE];
    let mut rtts = Vec::with_capacity(1 << 20);
    loop {
        let now = phase.load(Ordering::Relaxed);
        if now == STOP {
            return Ok(rtts);
        }
        let sent = Instant::now();
        stream.write_all(&request)?;
        stream.read_exact(&mut reply)?;
        if now == MEASURE {
            rtts.push(sent.elapsed().as_nanos() as u64);
        }
    }
}
