//! An ionotify arm fires once. `POLLARM` re-arms it, or, if data is still
//! unread, returns the met condition and arms nothing.

use std::{
    io::{self, Read, Write},
    net::{TcpListener, TcpStream},
    os::fd::AsRawFd,
};

use crate::{
    ffi,
    utils::{check, print_pulse, recv_pulse, READY},
};

/// `POLLARM` for INPUT; returns the conditions already met (0 = armed).
fn pollarm(fd: i32, ev: &ffi::SigEvent) -> io::Result<i32> {
    // SAFETY: `fd` is open, `ev` is registered and outlives the call.
    check(
        unsafe { ffi::ionotify(fd, ffi::NOTIFY_ACTION_POLLARM, ffi::NOTIFY_COND_INPUT, ev) },
        "ionotify",
    )
}

pub fn run(chid: i32, coid: i32) -> io::Result<()> {
    println!("[rearm] one-shot + re-arm");
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let mut client = TcpStream::connect(listener.local_addr()?)?;
    let (mut server, _) = listener.accept()?;
    server.set_nonblocking(true)?;
    let fd = server.as_raw_fd();

    let mut ev = ffi::SigEvent::pulse(coid, 10, READY, 0);
    // SAFETY: `ev` has the C sigevent layout; the kernel may rewrite it.
    check(
        unsafe { libc::MsgRegisterEvent((&mut ev as *mut ffi::SigEvent).cast(), fd) },
        "MsgRegisterEvent",
    )?;

    // Arm, write: the first pulse.
    println!(
        "  arm on empty socket: POLLARM = {:#x} (want 0x0)",
        pollarm(fd, &ev)?
    );
    client.write_all(b"a")?;
    print!("  first write, want code = 2 → ");
    print_pulse(recv_pulse(chid, 1000)?.as_ref());

    // Write again without re-arming: delivery disarmed it.
    client.write_all(b"b")?;
    print!("  second write without re-arm, want NO PULSE → ");
    print_pulse(recv_pulse(chid, 500)?.as_ref());

    // Re-arm while data is unread: ready now, so nothing is armed.
    println!(
        "  re-arm with unread data: POLLARM = {:#x} (want 0x10000000)",
        pollarm(fd, &ev)?
    );
    print!("  after ready-now re-arm, want NO PULSE → ");
    print_pulse(recv_pulse(chid, 500)?.as_ref());

    // Drain, re-arm on an empty socket, write: the second pulse.
    let mut buf = [0u8; 64];
    let n = server.read(&mut buf)?;
    println!("  drained {n} bytes (want 2)");
    println!(
        "  re-arm on empty socket: POLLARM = {:#x} (want 0x0)",
        pollarm(fd, &ev)?
    );
    client.write_all(b"c")?;
    print!("  third write, want code = 2 → ");
    print_pulse(recv_pulse(chid, 1000)?.as_ref());

    // SAFETY: `ev` was registered above.
    check(
        unsafe { libc::MsgUnregisterEvent((&ev as *const ffi::SigEvent).cast()) },
        "MsgUnregisterEvent",
    )?;

    Ok(())
}
