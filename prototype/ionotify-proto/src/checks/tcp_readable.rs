//! Data arriving on an armed TCP socket makes io-sock deliver our pulse.
//!
//! The event must be registered with `MsgRegisterEvent(&ev, fd)`: the socket fd
//! names the server allowed to deliver it. Registering with our own coid
//! succeeds, but the pulse is then never delivered.

use std::{
    io::{self, Write},
    net::{TcpListener, TcpStream},
    os::fd::AsRawFd,
};

use crate::{
    ffi,
    utils::{check, print_pulse, recv_pulse, READY},
};

pub fn run(chid: i32, coid: i32) -> io::Result<()> {
    println!("[tcp_readable] socket data → pulse");
    // Both ends in this process: we watch `server` and write on `client`.
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let mut client = TcpStream::connect(listener.local_addr()?)?;
    let (server, _) = listener.accept()?;
    let fd = server.as_raw_fd();

    // Deliver a READY pulse to our channel, value = slot 0 (the only socket here).
    let slot = 0usize;
    let mut ev = ffi::SigEvent::pulse(coid, 10, READY, slot);

    // Allow the server behind `fd` (io-sock) to deliver this event.
    // SAFETY: `ev` has the C sigevent layout; the kernel may rewrite it.
    check(
        unsafe { libc::MsgRegisterEvent((&mut ev as *mut ffi::SigEvent).cast(), fd) },
        "MsgRegisterEvent",
    )?;

    // SAFETY: `fd` is open, `ev` is registered and outlives the call.
    let ret = check(
        unsafe { ffi::ionotify(fd, ffi::NOTIFY_ACTION_POLLARM, ffi::NOTIFY_COND_INPUT, &ev) },
        "ionotify",
    )?;
    println!("  arm on empty socket: POLLARM = {ret:#x} (want 0x0 = armed)");

    // Make it readable.
    client.write_all(b"hi")?;
    print!("  data arrives, want code = 2, value = 0x0 → ");
    print_pulse(recv_pulse(chid, 1000)?.as_ref());

    // SAFETY: `ev` was registered above.
    check(
        unsafe { libc::MsgUnregisterEvent((&ev as *const ffi::SigEvent).cast()) },
        "MsgUnregisterEvent",
    )?;
    Ok(())
}
