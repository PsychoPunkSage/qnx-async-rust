//! Stress test for mio on QNX: many TCP connections with random traffic for a
//! set time, checking that no wakeup is lost and no data is corrupted.
//!
//! Usage: `stress [seconds] [connections]` (defaults: 10 s, 100 connections).
//!
//! Every connection has an echo thread on the far end. The mio side writes
//! random-sized bursts of a known byte pattern and reads the echo back, so
//! every byte is checked. A thread wakes the `Waker` at random times.
//!
//! - **Lost wakeup:** a connection with data in flight that makes no progress
//!   for `STALL`. Each connection only moves when its own events arrive, so
//!   traffic on other connections can't hide it.
//! - **Duplicate wakeup:** more waker events than wakes, or a readable event
//!   with nothing to read (counted; mio allows spurious events).
//!
//! Each registration uses one registered event, capped by
//! `RLIMIT_SIGEVENT_NP` (256 by default), so keep `connections` below ~250.

use std::io::{self, ErrorKind, Read, Write};
use std::net::{self, TcpListener};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use mio::net::TcpStream;
use mio::{Events, Interest, Poll, Token, Waker};

/// No progress for this long, with data in flight, counts as a lost wakeup.
const STALL: Duration = Duration::from_secs(5);
const WAKER: Token = Token(usize::MAX);
/// Largest burst queued on a connection at once.
const MAX_BURST: u64 = 64 * 1024;
/// New bursts started per loop iteration.
const BURSTS_PER_ROUND: usize = 4;

/// xorshift64: deterministic and fast; only used to pick sizes and timings.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x % n
    }
}

/// The byte at stream position `pos` of connection `conn`.
fn pattern(conn: usize, pos: u64) -> u8 {
    (pos.wrapping_mul(31) ^ conn as u64) as u8
}

struct Conn {
    stream: TcpStream,
    /// Bytes queued so far; everything below it is to be sent and echoed.
    target: u64,
    sent: u64,
    received: u64,
    /// Last time any byte moved in either direction.
    progress: Instant,
}

#[derive(Default)]
struct Stats {
    events: u64,
    readable: u64,
    writable: u64,
    spurious_reads: u64,
    wakes: u64,
    bursts: u64,
}

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let secs: u64 = args.next().map_or(10, |s| s.parse().expect("seconds"));
    let n: usize = args.next().map_or(100, |s| s.parse().expect("connections"));

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(256);
    let waker = Arc::new(Waker::new(poll.registry(), WAKER)?);

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let addr = listener.local_addr()?;
    let mut conns = Vec::with_capacity(n);
    for i in 0..n {
        let mut stream = TcpStream::connect(addr)?;
        let (peer, _) = listener.accept()?;
        thread::spawn(move || echo(peer));
        poll.registry().register(
            &mut stream,
            Token(i),
            Interest::READABLE | Interest::WRITABLE,
        )?;
        conns.push(Conn {
            stream,
            target: 0,
            sent: 0,
            received: 0,
            progress: Instant::now(),
        });
    }

    let stop = Arc::new(AtomicBool::new(false));
    let wakes_sent = Arc::new(AtomicU64::new(0));
    let waker_thread = {
        let (stop, wakes_sent, waker) = (stop.clone(), wakes_sent.clone(), waker.clone());
        thread::spawn(move || {
            let mut rng = Rng(7);
            while !stop.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(rng.below(20)));
                waker.wake().expect("wake");
                wakes_sent.fetch_add(1, Ordering::Relaxed);
            }
        })
    };

    println!("stress: {secs} s, {n} connections");
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut stats = Stats::default();
    let mut rbuf = vec![0; 64 * 1024];
    let mut wbuf = vec![0; 64 * 1024];
    let start = Instant::now();
    let end = start + Duration::from_secs(secs);
    let mut draining = false;

    loop {
        if !draining && Instant::now() >= end {
            // Stop queuing new data; finish what is in flight.
            draining = true;
            stop.store(true, Ordering::Relaxed);
        }
        if draining && conns.iter().all(|c| c.received == c.target) {
            break;
        }

        if !draining {
            for _ in 0..BURSTS_PER_ROUND {
                let i = rng.below(n as u64) as usize;
                let conn = &mut conns[i];
                conn.target += 1 + rng.below(MAX_BURST);
                // Writes may be tried at any time; WouldBlock waits for an event.
                write_some(i, conn, &mut wbuf)?;
                stats.bursts += 1;
            }
        }

        poll.poll(&mut events, Some(Duration::from_millis(100)))?;
        for event in events.iter() {
            stats.events += 1;
            if event.token() == WAKER {
                stats.wakes += 1;
                continue;
            }
            let i = event.token().0;
            let conn = &mut conns[i];
            if event.is_writable() {
                stats.writable += 1;
                write_some(i, conn, &mut wbuf)?;
            }
            if event.is_readable() {
                stats.readable += 1;
                if read_some(i, conn, &mut rbuf)? == 0 {
                    stats.spurious_reads += 1;
                }
            }
        }

        for (i, conn) in conns.iter().enumerate() {
            if conn.received < conn.target && conn.progress.elapsed() >= STALL {
                return Err(io::Error::new(
                    ErrorKind::TimedOut,
                    format!(
                        "lost wakeup: connection {i} made no progress for {STALL:?} \
                         (sent {} of {}, received {})",
                        conn.sent, conn.target, conn.received
                    ),
                ));
            }
        }
    }

    waker_thread.join().expect("waker thread");
    // Wakes sent after the last poll are still queued; take them.
    poll.poll(&mut events, Some(Duration::from_millis(100)))?;
    stats.wakes += events.iter().filter(|e| e.token() == WAKER).count() as u64;
    let wakes_sent = wakes_sent.load(Ordering::Relaxed);
    if stats.wakes > wakes_sent {
        return Err(io::Error::other(format!(
            "duplicate wakeup: {} waker events for {wakes_sent} wakes",
            stats.wakes
        )));
    }
    if wakes_sent > 0 && stats.wakes == 0 {
        return Err(io::Error::other("lost wakeup: no waker event at all"));
    }

    let bytes: u64 = conns.iter().map(|c| c.received).sum();
    println!("OK after {:.1} s", start.elapsed().as_secs_f64());
    println!(
        "  echoed {:.1} MiB in {} bursts",
        bytes as f64 / (1024.0 * 1024.0),
        stats.bursts
    );
    println!(
        "  events {} (readable {}, writable {}, waker {} for {wakes_sent} wakes)",
        stats.events, stats.readable, stats.writable, stats.wakes
    );
    println!(
        "  readable events with nothing to read: {}",
        stats.spurious_reads
    );
    Ok(())
}

/// The far end: send back whatever arrives.
fn echo(mut peer: net::TcpStream) {
    let mut buf = [0; 16 * 1024];
    loop {
        match peer.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(len) => {
                if peer.write_all(&buf[..len]).is_err() {
                    return;
                }
            }
        }
    }
}

/// Write queued bytes until all are sent or the socket would block.
fn write_some(i: usize, conn: &mut Conn, buf: &mut [u8]) -> io::Result<()> {
    while conn.sent < conn.target {
        let len = ((conn.target - conn.sent) as usize).min(buf.len());
        for (k, byte) in buf[..len].iter_mut().enumerate() {
            *byte = pattern(i, conn.sent + k as u64);
        }
        match conn.stream.write(&buf[..len]) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(written) => {
                conn.sent += written as u64;
                conn.progress = Instant::now();
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Read until the socket would block, checking every byte. Returns how many
/// bytes were read.
fn read_some(i: usize, conn: &mut Conn, buf: &mut [u8]) -> io::Result<u64> {
    let mut total = 0;
    loop {
        match conn.stream.read(buf) {
            Ok(0) => {
                return Err(io::Error::new(
                    ErrorKind::UnexpectedEof,
                    format!("connection {i}: echo side closed"),
                ))
            }
            Ok(len) => {
                for (k, &byte) in buf[..len].iter().enumerate() {
                    let pos = conn.received + k as u64;
                    if byte != pattern(i, pos) {
                        return Err(io::Error::new(
                            ErrorKind::InvalidData,
                            format!("connection {i}: wrong byte at {pos}"),
                        ));
                    }
                }
                conn.received += len as u64;
                if conn.received > conn.sent {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        format!("connection {i}: received more than sent"),
                    ));
                }
                conn.progress = Instant::now();
                total += len as u64;
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(total),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}
