//! How the extended conditions (`_NOTIFY_COND_EXTEN` + `_NOTIFY_CONDE_*`)
//! report errors and hang-ups, which mio needs for `is_error`,
//! `is_read_closed` and `is_write_closed`.
//!
//! `sys/iomgr.h` says that when extended conditions fire on an armed event, the
//! negative of the satisfied conditions is returned in the event's code. So
//! the pulse code is printed too: it may not be READY any more.

use std::{
    io, mem,
    net::{Shutdown, TcpListener, TcpStream},
    os::fd::AsRawFd,
};

use crate::{
    ffi,
    utils::{check, print_pulse, recv_pulse, READY},
};

/// Every basic and extended condition we care about.
const ALL: i32 = ffi::NOTIFY_COND_INPUT
    | ffi::NOTIFY_COND_OUTPUT
    | ffi::NOTIFY_COND_EXTEN
    | ffi::NOTIFY_CONDE_RDNORM
    | ffi::NOTIFY_CONDE_WRNORM
    | ffi::NOTIFY_CONDE_ERR
    | ffi::NOTIFY_CONDE_HUP;

/// Hang-up and error only.
const HUP_ERR: i32 = ffi::NOTIFY_COND_EXTEN | ffi::NOTIFY_CONDE_ERR | ffi::NOTIFY_CONDE_HUP;

/// Name the bits set in an `ionotify()` return value.
fn names(bits: i32) -> String {
    let table = [
        (ffi::NOTIFY_COND_INPUT, "INPUT"),
        (ffi::NOTIFY_COND_OUTPUT, "OUTPUT"),
        (ffi::NOTIFY_COND_OBAND, "OBAND"),
        (ffi::NOTIFY_COND_EXTEN, "EXTEN"),
        (ffi::NOTIFY_CONDE_RDNORM, "RDNORM"),
        (ffi::NOTIFY_CONDE_WRNORM, "WRNORM"),
        (ffi::NOTIFY_CONDE_ERR, "ERR"),
        (ffi::NOTIFY_CONDE_HUP, "HUP"),
        (ffi::NOTIFY_CONDE_NVAL, "NVAL"),
    ];
    let set: Vec<&str> = table
        .iter()
        .filter(|(bit, _)| bits & bit != 0)
        .map(|(_, name)| *name)
        .collect();
    let rest = table.iter().fold(bits, |b, (bit, _)| b & !bit);
    let mut out = if set.is_empty() {
        "none".to_string()
    } else {
        set.join("|")
    };
    if rest != 0 {
        out.push_str(&format!(" + unknown {rest:#x}"));
    }
    format!("{bits:#x} ({out})")
}

/// `ionotify()` with `action`; prints the result or the error.
fn notify(fd: i32, action: i32, conds: i32, ev: Option<&ffi::SigEvent>) -> Option<i32> {
    let ev = ev.map_or(std::ptr::null(), |ev| ev as *const ffi::SigEvent);
    // SAFETY: `fd` is open; `ev` is null (POLL) or registered and outlives the call.
    let ret = unsafe { ffi::ionotify(fd, action, conds, ev) };
    if ret == -1 {
        println!("-1, {}", io::Error::last_os_error());
        None
    } else {
        println!("{}", names(ret));
        Some(ret)
    }
}

fn poll(fd: i32, conds: i32) {
    notify(fd, ffi::NOTIFY_ACTION_POLL, conds, None);
}

fn pollarm(fd: i32, conds: i32, ev: &ffi::SigEvent) -> Option<i32> {
    notify(fd, ffi::NOTIFY_ACTION_POLLARM, conds, Some(ev))
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

/// A connected (client, server) pair over loopback.
fn pair() -> io::Result<(TcpStream, TcpStream)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let client = TcpStream::connect(listener.local_addr()?)?;
    let (server, _) = listener.accept()?;
    Ok((client, server))
}

/// Arm `conds` on the server side of a fresh pair, run `act` on the pair, then
/// report the pulse (if any) and what POLL sees afterwards.
fn arm_then(
    chid: i32,
    coid: i32,
    title: &str,
    conds: i32,
    act: impl FnOnce(TcpStream, &TcpStream),
) -> io::Result<()> {
    println!("  {title}");
    let (client, server) = pair()?;
    let fd = server.as_raw_fd();
    let ev = register(coid, fd)?;

    print!("    POLLARM {}: ", names(conds));
    pollarm(fd, conds, &ev);

    act(client, &server);
    print!("    pulse? → ");
    print_pulse(recv_pulse(chid, 1000)?.as_ref());
    print!("    POLL all after: ");
    poll(fd, ALL);

    unregister(&ev)?;
    Ok(())
}

/// Non-blocking connect to a loopback port nobody listens on.
fn refused_connect(chid: i32, coid: i32) -> io::Result<()> {
    println!("  connect to a closed port (non-blocking)");
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
    println!(
        "    connect: {}",
        if ret == 0 {
            "0".to_string()
        } else {
            format!("-1, {err}")
        }
    );

    if ret == -1 && err.raw_os_error() == Some(libc::EINPROGRESS) {
        let ev = register(coid, fd)?;
        print!("    POLLARM {}: ", names(ffi::NOTIFY_COND_OUTPUT | HUP_ERR));
        pollarm(fd, ffi::NOTIFY_COND_OUTPUT | HUP_ERR, &ev);
        print!("    pulse? → ");
        print_pulse(recv_pulse(chid, 1000)?.as_ref());
        print!("    POLL all after: ");
        poll(fd, ALL);
        unregister(&ev)?;
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
    println!("[exten] errors and hang-ups through the extended conditions");

    println!("  idle connected socket");
    let (_client, server) = pair()?;
    print!("    POLL all: ");
    poll(server.as_raw_fd(), ALL);
    print!("    POLL HUP|ERR only: ");
    poll(server.as_raw_fd(), HUP_ERR);
    drop(server);

    arm_then(
        chid,
        coid,
        "INPUT + HUP|ERR armed, other end closes",
        ffi::NOTIFY_COND_INPUT | HUP_ERR,
        |client, _| drop(client),
    )?;

    arm_then(
        chid,
        coid,
        "HUP|ERR only armed, other end closes",
        HUP_ERR,
        |client, _| drop(client),
    )?;

    // Like a writable socket mio already reported: OUTPUT isn't armed any more.
    arm_then(
        chid,
        coid,
        "INPUT + HUP|ERR armed, own shutdown(Both)",
        ffi::NOTIFY_COND_INPUT | HUP_ERR,
        |_client, server| {
            let _ = server.shutdown(Shutdown::Both);
        },
    )?;

    arm_then(
        chid,
        coid,
        "HUP|ERR only armed, own shutdown(Write)",
        HUP_ERR,
        |_client, server| {
            let _ = server.shutdown(Shutdown::Write);
        },
    )?;

    refused_connect(chid, coid)?;
    Ok(())
}
