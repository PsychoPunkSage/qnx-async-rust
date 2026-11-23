//! Does closing the other end wake an armed reader? Tested for a pipe (whose
//! writer close gives no POLLHUP through `poll()`) and for a TCP socket.

use std::{
    io,
    net::{TcpListener, TcpStream},
    os::fd::AsRawFd,
};

use crate::{
    ffi,
    utils::{check, print_pulse, recv_pulse, READY},
};

/// Register a READY event for `fd`, arm INPUT, run `close_peer`, then report
/// whether a pulse came and what `POLL` says afterwards.
fn arm_and_close(chid: i32, coid: i32, fd: i32, close_peer: impl FnOnce()) -> io::Result<()> {
    let mut ev = ffi::SigEvent::pulse(coid, 10, READY, 0);
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
    println!("  arm on empty reader: POLLARM = {ret:#x} (want 0x0)");

    close_peer();
    print!("  other end closed, pulse? → ");
    print_pulse(recv_pulse(chid, 1000)?.as_ref());

    // POLL never arms; it only reports the conditions met now.
    // SAFETY: `fd` is open; POLL takes no event.
    let ret = check(
        unsafe {
            ffi::ionotify(
                fd,
                ffi::NOTIFY_ACTION_POLL,
                ffi::NOTIFY_COND_INPUT,
                std::ptr::null(),
            )
        },
        "ionotify",
    )?;
    println!("  after close: POLL = {ret:#x} (0x10000000 = readable, i.e. EOF visible)");

    // SAFETY: `ev` was registered above.
    check(
        unsafe { libc::MsgUnregisterEvent((&ev as *const ffi::SigEvent).cast()) },
        "MsgUnregisterEvent",
    )?;
    Ok(())
}

pub fn run(chid: i32, coid: i32) -> io::Result<()> {
    println!("[peer_close] other end closes");

    println!("  pipe:");
    let mut fds = [0; 2];
    // SAFETY: `fds` has room for the two fds pipe() writes.
    check(unsafe { libc::pipe(fds.as_mut_ptr()) }, "pipe")?;
    let (read_fd, write_fd) = (fds[0], fds[1]);
    arm_and_close(chid, coid, read_fd, || {
        // SAFETY: `write_fd` is open and owned here; nothing uses it afterwards.
        unsafe { libc::close(write_fd) };
    })?;
    // SAFETY: `read_fd` is open and owned here.
    unsafe { libc::close(read_fd) };

    println!("  tcp:");
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let client = TcpStream::connect(listener.local_addr()?)?;
    let (server, _) = listener.accept()?;
    arm_and_close(chid, coid, server.as_raw_fd(), || drop(client))?;

    Ok(())
}
