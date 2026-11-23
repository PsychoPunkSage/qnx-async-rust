//! `RLIMIT_SIGEVENT_NP` caps how many events a process can register, and so how
//! many fds one selector can watch. Reads the limit, registers events until
//! `MsgRegisterEvent()` fails, then tries raising the limit and counts again.

use std::{
    io,
    net::{TcpListener, TcpStream},
    os::fd::AsRawFd,
};

use crate::{
    ffi,
    utils::{check, READY},
};

/// Stop here if no limit is hit, so the check always ends.
const MAX_TRY: usize = 100_000;
/// Limit to try raising RLIMIT_SIGEVENT_NP to (enough for 1000+ sockets).
const RAISE_TO: libc::rlim_t = 4096;

fn show(v: libc::rlim_t) -> String {
    if v == libc::RLIM_INFINITY {
        "unlimited".into()
    } else {
        v.to_string()
    }
}

/// Register events on one socket until `MsgRegisterEvent()` fails, print the
/// count, then unregister them all.
fn register_until_full(coid: i32) -> io::Result<()> {
    // One socket is enough: every MsgRegisterEvent call takes one slot, whatever the fd.
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let _client = TcpStream::connect(listener.local_addr()?)?;
    let (server, _) = listener.accept()?;
    let fd = server.as_raw_fd();

    // Boxed so each event keeps a fixed address: the kernel rewrites it, and
    // MsgUnregisterEvent needs that same rewritten copy.
    let mut events: Vec<Box<ffi::SigEvent>> = Vec::new();
    let mut error = None;
    while events.len() < MAX_TRY {
        let mut ev = Box::new(ffi::SigEvent::pulse(coid, 10, READY, events.len()));
        // SAFETY: `ev` has the C sigevent layout; the kernel may rewrite it.
        let ret = unsafe { libc::MsgRegisterEvent((&mut *ev as *mut ffi::SigEvent).cast(), fd) };
        if ret == -1 {
            error = Some(io::Error::last_os_error());
            break;
        }
        events.push(ev);
    }

    match error {
        Some(e) => println!("    registered {} events, then failed: {e}", events.len()),
        None => println!("    registered {MAX_TRY} events, no limit hit"),
    }

    for ev in &events {
        // SAFETY: each `ev` was registered above.
        unsafe { libc::MsgUnregisterEvent((&**ev as *const ffi::SigEvent).cast()) };
    }
    println!("    unregistered {}", events.len());
    Ok(())
}

pub fn run(coid: i32) -> io::Result<()> {
    println!("[event_limits] RLIMIT_SIGEVENT_NP");

    // Soft (`rlim_cur`) is enforced; hard (`rlim_max`) is the ceiling a process
    // may raise its soft limit to.
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a valid, writable rlimit.
    check(
        unsafe { libc::getrlimit(ffi::RLIMIT_SIGEVENT_NP, &mut limit) },
        "getrlimit",
    )?;
    println!(
        "  limit: soft = {}, hard = {}",
        show(limit.rlim_cur),
        show(limit.rlim_max)
    );

    println!("  at the default limit:");
    register_until_full(coid)?;

    // Try raising both limits; above the hard limit this needs privileges.
    let raised = libc::rlimit {
        rlim_cur: RAISE_TO,
        rlim_max: RAISE_TO,
    };
    // SAFETY: `raised` is a valid rlimit that outlives the call.
    let ret = unsafe { libc::setrlimit(ffi::RLIMIT_SIGEVENT_NP, &raised) };
    if ret == -1 {
        println!(
            "  raise to {RAISE_TO}: failed: {}",
            io::Error::last_os_error()
        );
    } else {
        println!("  raise to {RAISE_TO}: ok");
        register_until_full(coid)?;
    }
    Ok(())
}
