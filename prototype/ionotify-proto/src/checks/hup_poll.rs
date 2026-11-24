//! Hang-ups and errors without extended conditions: arm only INPUT/OUTPUT,
//! wait for the pulse, then ask `poll()` with a zero timeout for that one fd.
//!
//! Extended conditions are rejected by `ionotify()` with EINVAL (see `exten`),
//! so `poll()` is the public way to read POLLHUP / POLLERR. Here it never
//! waits: the pulse does the waiting, and `poll()` only reads the flags of an
//! fd that already pulsed.

use std::{
    io, mem,
    net::{Shutdown, TcpListener, TcpStream},
    os::fd::AsRawFd,
};

use crate::{
    ffi,
    utils::{check, print_pulse, recv_pulse, READY},
};

/// Name the basic `ionotify()` conditions set in `bits`.
fn conds(bits: i32) -> String {
    let table = [
        (ffi::NOTIFY_COND_INPUT, "INPUT"),
        (ffi::NOTIFY_COND_OUTPUT, "OUTPUT"),
        (ffi::NOTIFY_COND_OBAND, "OBAND"),
    ];
    let set: Vec<&str> = table
        .iter()
        .filter(|(bit, _)| bits & bit != 0)
        .map(|(_, name)| *name)
        .collect();
    if set.is_empty() {
        "none".to_string()
    } else {
        set.join("|")
    }
}

/// Name the `poll()` flags set in `revents`.
fn revents(bits: libc::c_short) -> String {
    let table = [
        (libc::POLLIN, "POLLIN"),
        (libc::POLLOUT, "POLLOUT"),
        (libc::POLLPRI, "POLLPRI"),
        (libc::POLLRDNORM, "POLLRDNORM"),
        (libc::POLLWRNORM, "POLLWRNORM"),
        (libc::POLLERR, "POLLERR"),
        (libc::POLLHUP, "POLLHUP"),
        (libc::POLLNVAL, "POLLNVAL"),
    ];
    let set: Vec<&str> = table
        .iter()
        .filter(|(bit, _)| bits & bit != 0)
        .map(|(_, name)| *name)
        .collect();
    let shown = if set.is_empty() {
        "none".to_string()
    } else {
        set.join("|")
    };
    format!("{bits:#x} ({shown})")
}

/// `poll()` one fd for reading and writing with a zero timeout; print `revents`.
fn poll_now(fd: i32) -> io::Result<()> {
    let mut pfd = libc::pollfd {
        fd,
        events: libc::POLLIN | libc::POLLOUT,
        revents: 0,
    };
    // SAFETY: `pfd` is one valid pollfd; a zero timeout never blocks.
    check(unsafe { libc::poll(&mut pfd, 1, 0) }, "poll")?;
    println!("    poll(fd, 0): {}", revents(pfd.revents));
    Ok(())
}

fn register(coid: i32, fd: i32) -> io::Result<ffi::SigEvent> {
    let mut ev = ffi::SigEvent::pulse(coid, 10, READY, 0);
    // SAFETY: `ev` has the C sigevent layout; the kernel may rewrite it.
    check(
        unsafe { libc::MsgRegisterEvent((&mut ev as *mut ffi::SigEvent).cast(), fd) },
        "MsgRegisterEvent",
    )?;
    Ok(ev)
}

fn unregister(ev: &ffi::SigEvent) -> io::Result<()> {
    // SAFETY: `ev` was registered by `register`.
    check(
        unsafe { libc::MsgUnregisterEvent((ev as *const ffi::SigEvent).cast()) },
        "MsgUnregisterEvent",
    )?;
    Ok(())
}

/// POLLARM `want` on `fd`. A non-zero return means already met: nothing is
/// armed and no pulse will come.
fn pollarm(fd: i32, want: i32, ev: &ffi::SigEvent) -> io::Result<()> {
    // SAFETY: `fd` is open and `ev` is registered for it and outlives the call.
    let met = check(
        unsafe { ffi::ionotify(fd, ffi::NOTIFY_ACTION_POLLARM, want, ev) },
        "ionotify",
    )?;
    println!(
        "    POLLARM {}: met now = {}",
        conds(want),
        conds(met & want)
    );
    Ok(())
}

/// Wait for one pulse, then show what `poll()` says about `fd`.
fn pulse_then_poll(chid: i32, fd: i32) -> io::Result<()> {
    print!("    pulse? → ");
    print_pulse(recv_pulse(chid, 1000)?.as_ref());
    poll_now(fd)
}

/// A connected (client, server) pair over loopback.
fn pair() -> io::Result<(TcpStream, TcpStream)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let client = TcpStream::connect(listener.local_addr()?)?;
    let (server, _) = listener.accept()?;
    Ok((client, server))
}

/// Arm `want` on the server side of a fresh pair, run `act`, then report the
/// pulse and the `poll()` flags. The client stays open until the end unless
/// `act` takes it out of the `Option`.
fn arm_then(
    chid: i32,
    coid: i32,
    title: &str,
    want: i32,
    act: impl FnOnce(&mut Option<TcpStream>, &TcpStream),
) -> io::Result<()> {
    println!("  {title}");
    let (client, server) = pair()?;
    let mut client = Some(client);
    let fd = server.as_raw_fd();
    let ev = register(coid, fd)?;
    pollarm(fd, want, &ev)?;
    act(&mut client, &server);
    pulse_then_poll(chid, fd)?;
    unregister(&ev)?;
    drop(client);
    Ok(())
}

/// The way mio arms a fresh socket: POLLARM INPUT|OUTPUT (OUTPUT is met, so
/// nothing is armed), then POLLARM INPUT alone. Then `shutdown(Both)` on the
/// client side (`client_side`) or on the accepted side.
fn mio_like(chid: i32, coid: i32, client_side: bool, two_step: bool) -> io::Result<()> {
    println!(
        "  {} side, {}, own shutdown(Both)",
        if client_side { "client" } else { "server" },
        if two_step {
            "POLLARM INPUT|OUTPUT then INPUT"
        } else {
            "POLLARM INPUT once"
        }
    );
    let (client, server) = pair()?;
    let (me, _peer) = if client_side {
        (client, server)
    } else {
        (server, client)
    };
    let fd = me.as_raw_fd();
    let ev = register(coid, fd)?;
    if two_step {
        pollarm(fd, ffi::NOTIFY_COND_INPUT | ffi::NOTIFY_COND_OUTPUT, &ev)?;
    }
    pollarm(fd, ffi::NOTIFY_COND_INPUT, &ev)?;
    let _ = me.shutdown(Shutdown::Both);
    pulse_then_poll(chid, fd)?;
    unregister(&ev)
}

/// Like mio: a non-blocking `connect()` to a live listener, a pulse with
/// priority `prio`, POLLARM INPUT|OUTPUT then INPUT, then own `shutdown(Both)`.
fn nonblocking_client(chid: i32, coid: i32, prio: i16) -> io::Result<()> {
    println!("  non-blocking connect client, pulse prio {prio}, own shutdown(Both)");
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    // SAFETY: plain syscall, no pointers.
    let fd = check(
        unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) },
        "socket",
    )?;
    // SAFETY: `fd` is a socket we own.
    check(
        unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) },
        "fcntl",
    )?;
    // SAFETY: all-zero is a valid sockaddr_in; the fields are set below.
    let mut addr: libc::sockaddr_in = unsafe { mem::zeroed() };
    addr.sin_len = mem::size_of::<libc::sockaddr_in>() as u8;
    addr.sin_family = libc::AF_INET as libc::sa_family_t;
    addr.sin_port = port.to_be();
    addr.sin_addr.s_addr = u32::from_ne_bytes([127, 0, 0, 1]);
    // SAFETY: `addr` is a valid sockaddr_in of the size passed.
    let ret = unsafe {
        libc::connect(
            fd,
            (&addr as *const libc::sockaddr_in).cast(),
            mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        )
    };
    println!(
        "    connect: {}",
        if ret == 0 {
            "0".to_string()
        } else {
            format!("-1, {}", io::Error::last_os_error())
        }
    );
    let (_peer, _) = listener.accept()?;

    let mut ev = ffi::SigEvent::pulse(coid, prio, READY, 0);
    // SAFETY: `ev` has the C sigevent layout; the kernel may rewrite it.
    check(
        unsafe { libc::MsgRegisterEvent((&mut ev as *mut ffi::SigEvent).cast(), fd) },
        "MsgRegisterEvent",
    )?;
    pollarm(fd, ffi::NOTIFY_COND_INPUT | ffi::NOTIFY_COND_OUTPUT, &ev)?;
    pollarm(fd, ffi::NOTIFY_COND_INPUT, &ev)?;
    // SAFETY: `fd` is a connected socket we own.
    check(unsafe { libc::shutdown(fd, libc::SHUT_RDWR) }, "shutdown")?;
    pulse_then_poll(chid, fd)?;
    unregister(&ev)?;
    // SAFETY: `fd` is ours and not used afterwards.
    unsafe { libc::close(fd) };
    Ok(())
}

/// Arm INPUT, call `poll(fd, 0)`, then `shutdown(Both)`: does `poll()` cancel
/// the arming?
fn poll_after_arm(chid: i32, coid: i32) -> io::Result<()> {
    println!("  INPUT armed, then poll(fd, 0), then own shutdown(Both)");
    let (_client, server) = pair()?;
    let fd = server.as_raw_fd();
    let ev = register(coid, fd)?;
    pollarm(fd, ffi::NOTIFY_COND_INPUT, &ev)?;
    poll_now(fd)?;
    let _ = server.shutdown(Shutdown::Both);
    pulse_then_poll(chid, fd)?;
    unregister(&ev)
}

/// Non-blocking connect to a loopback port nobody listens on, with OUTPUT armed.
fn refused_connect(chid: i32, coid: i32) -> io::Result<()> {
    println!("  OUTPUT armed, connect to a closed port (non-blocking)");
    // SAFETY: plain syscall, no pointers.
    let fd = check(
        unsafe { libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0) },
        "socket",
    )?;
    // SAFETY: `fd` is a socket we own.
    check(
        unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) },
        "fcntl",
    )?;

    // SAFETY: all-zero is a valid sockaddr_in; the fields are set below.
    let mut addr: libc::sockaddr_in = unsafe { mem::zeroed() };
    addr.sin_len = mem::size_of::<libc::sockaddr_in>() as u8;
    addr.sin_family = libc::AF_INET as libc::sa_family_t;
    addr.sin_port = 58381_u16.to_be();
    addr.sin_addr.s_addr = u32::from_ne_bytes([127, 0, 0, 1]);
    // SAFETY: `addr` is a valid sockaddr_in of the size passed.
    let ret = unsafe {
        libc::connect(
            fd,
            (&addr as *const libc::sockaddr_in).cast(),
            mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        )
    };
    let err = io::Error::last_os_error();
    if ret == 0 {
        println!("    connect: 0");
    } else {
        println!("    connect: -1, {err}");
    }

    if ret == -1 && err.raw_os_error() == Some(libc::EINPROGRESS) {
        let ev = register(coid, fd)?;
        pollarm(fd, ffi::NOTIFY_COND_OUTPUT, &ev)?;
        pulse_then_poll(chid, fd)?;
        unregister(&ev)?;
    } else {
        poll_now(fd)?;
    }

    let mut so_error: libc::c_int = 0;
    let mut len = mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: `so_error` and `len` are valid for writes of the sizes given.
    check(
        unsafe {
            libc::getsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_ERROR,
                (&mut so_error as *mut libc::c_int).cast(),
                &mut len,
            )
        },
        "getsockopt",
    )?;
    println!("    SO_ERROR: {}", io::Error::from_raw_os_error(so_error));

    // SAFETY: `fd` is ours and not used afterwards.
    unsafe { libc::close(fd) };
    Ok(())
}

pub fn run(chid: i32, coid: i32) -> io::Result<()> {
    println!("[hup_poll] hang-ups and errors via pulse + poll(fd, 0)");

    arm_then(
        chid,
        coid,
        "INPUT armed, other end closes",
        ffi::NOTIFY_COND_INPUT,
        |client, _| drop(client.take()),
    )?;

    // Like a socket whose writability mio already reported: OUTPUT isn't armed.
    arm_then(
        chid,
        coid,
        "INPUT armed, own shutdown(Both)",
        ffi::NOTIFY_COND_INPUT,
        |_client, server| {
            let _ = server.shutdown(Shutdown::Both);
        },
    )?;

    // Reading still works after this, so no pulse is the right answer.
    arm_then(
        chid,
        coid,
        "INPUT armed, own shutdown(Write)",
        ffi::NOTIFY_COND_INPUT,
        |_client, server| {
            let _ = server.shutdown(Shutdown::Write);
        },
    )?;

    refused_connect(chid, coid)?;

    // Which detail makes mio's client socket miss the shutdown pulse?
    mio_like(chid, coid, false, false)?;
    mio_like(chid, coid, false, true)?;
    mio_like(chid, coid, true, false)?;
    mio_like(chid, coid, true, true)?;
    nonblocking_client(chid, coid, 10)?;
    nonblocking_client(chid, coid, -1)?;
    poll_after_arm(chid, coid)
}
