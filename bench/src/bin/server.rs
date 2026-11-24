//! Echo server for the benchmark: one thread, one mio `Poll`, every message
//! echoed back as soon as it arrives. Built twice: once on mio's `poll()`
//! selector (`--cfg mio_unsupported_force_poll_poll`) and once on ionotify.
//!
//! Usage: `server <port>`. Runs until killed.

use std::io::{self, ErrorKind, Read, Write};
use std::net::SocketAddr;

use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};

const LISTENER: Token = Token(usize::MAX);
/// Enough for 1000+ connections plus the listener and stdio.
const MAX_FDS: u64 = 4096;

struct Conn {
    stream: TcpStream,
    /// Echo bytes not written yet (only when the send buffer was full).
    out: Vec<u8>,
    /// Whether WRITABLE is currently part of the registration.
    want_write: bool,
}

fn main() -> io::Result<()> {
    let port: u16 = std::env::args()
        .nth(1)
        .expect("usage: server <port>")
        .parse()
        .expect("port");
    bench::raise_limits(MAX_FDS)?;

    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(1024);
    let mut listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))?;
    poll.registry()
        .register(&mut listener, LISTENER, Interest::READABLE)?;

    let mut conns: Vec<Option<Conn>> = Vec::new();
    let mut buf = vec![0; 16 * 1024];
    loop {
        poll.poll(&mut events, None)?;
        for event in events.iter() {
            if event.token() == LISTENER {
                accept_all(&poll, &mut listener, &mut conns)?;
                continue;
            }
            let i = event.token().0;
            let Some(conn) = conns.get_mut(i).and_then(Option::as_mut) else {
                continue;
            };
            if !serve(&poll, Token(i), conn, &mut buf)? {
                poll.registry().deregister(&mut conn.stream)?;
                conns[i] = None;
            }
        }
    }
}

fn accept_all(
    poll: &Poll,
    listener: &mut TcpListener,
    conns: &mut Vec<Option<Conn>>,
) -> io::Result<()> {
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nodelay(true)?;
                let token = Token(conns.len());
                poll.registry()
                    .register(&mut stream, token, Interest::READABLE)?;
                conns.push(Some(Conn {
                    stream,
                    out: Vec::new(),
                    want_write: false,
                }));
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(()),
            Err(e) => return Err(e),
        }
    }
}

/// Read until WouldBlock and echo it back. Returns false once the peer closed.
fn serve(poll: &Poll, token: Token, conn: &mut Conn, buf: &mut [u8]) -> io::Result<bool> {
    let mut open = true;
    loop {
        match conn.stream.read(buf) {
            Ok(0) => {
                open = false;
                break;
            }
            Ok(len) => conn.out.extend_from_slice(&buf[..len]),
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if e.kind() == ErrorKind::ConnectionReset => return Ok(false),
            Err(e) => return Err(e),
        }
    }

    while !conn.out.is_empty() {
        match conn.stream.write(&conn.out) {
            Ok(written) => {
                conn.out.drain(..written);
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) if e.kind() == ErrorKind::BrokenPipe => return Ok(false),
            Err(e) if e.kind() == ErrorKind::ConnectionReset => return Ok(false),
            Err(e) => return Err(e),
        }
    }

    // Ask for WRITABLE only while there is something left to send.
    let want_write = !conn.out.is_empty();
    if open && want_write != conn.want_write {
        let interests = if want_write {
            Interest::READABLE | Interest::WRITABLE
        } else {
            Interest::READABLE
        };
        poll.registry()
            .reregister(&mut conn.stream, token, interests)?;
        conn.want_write = want_write;
    }
    Ok(open)
}
