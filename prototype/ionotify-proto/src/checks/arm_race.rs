//! Data present before arming is reported by `POLLARM`'s return value, not by a
//! pulse, so it is not lost as long as the caller uses that value.
//!
//! If any requested condition is already met, `POLLARM` arms none of them: after
//! `POLLARM(INPUT | OUTPUT)` returns OUTPUT, new input brings no pulse.

use std::{
    io::{self, Write},
    net::{TcpListener, TcpStream},
    os::fd::AsRawFd,
    thread::sleep,
    time::Duration,
};

use crate::{
    ffi,
    utils::{check, print_pulse, recv_pulse, READY},
};

/// `POLLARM` for `conds`; returns the conditions already met (0 = armed).
fn pollarm(fd: i32, conds: i32, ev: &ffi::SigEvent) -> io::Result<i32> {
    // SAFETY: `fd` is open, `ev` is registered and outlives the call.
    check(
        unsafe { ffi::ionotify(fd, ffi::NOTIFY_ACTION_POLLARM, conds, ev) },
        "ionotify",
    )
}

/// A connected (client, server) pair, with a READY event registered for the server fd.
fn setup(coid: i32) -> io::Result<(TcpStream, TcpStream, ffi::SigEvent)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let client = TcpStream::connect(listener.local_addr()?)?;
    let (server, _) = listener.accept()?;

    let mut ev = ffi::SigEvent::pulse(coid, 10, READY, 0);
    // SAFETY: `ev` has the C sigevent layout; the kernel may rewrite it.
    check(
        unsafe {
            libc::MsgRegisterEvent((&mut ev as *mut ffi::SigEvent).cast(), server.as_raw_fd())
        },
        "MsgRegisterEvent",
    )?;

    Ok((client, server, ev))
}

fn unregister(ev: &ffi::SigEvent) -> io::Result<()> {
    // SAFETY: `ev` was registered in setup().
    check(
        unsafe { libc::MsgUnregisterEvent((ev as *const ffi::SigEvent).cast()) },
        "MsgUnregisterEvent",
    )?;
    Ok(())
}

pub fn run(chid: i32, coid: i32) -> io::Result<()> {
    println!("[arm_race] ready before arm");
    let (input, output) = (ffi::NOTIFY_COND_INPUT, ffi::NOTIFY_COND_OUTPUT);

    // Data written before the first arm.
    let (mut client, server, ev) = setup(coid)?;
    client.write_all(b"early")?;
    sleep(Duration::from_millis(50)); // let the bytes reach the server end
    let ret = pollarm(server.as_raw_fd(), input, &ev)?;
    println!("  data before arm: POLLARM = {ret:#x} (want 0x10000000)");
    print!("  after ready-now arm, want NO PULSE → ");
    print_pulse(recv_pulse(chid, 500)?.as_ref());
    unregister(&ev)?;

    // INPUT | OUTPUT on a fresh socket: OUTPUT is already met, so is INPUT armed?
    let (mut client, server, ev) = setup(coid)?;
    let ret = pollarm(server.as_raw_fd(), input | output, &ev)?;
    println!("  INPUT|OUTPUT on fresh socket: POLLARM = {ret:#x} (want 0x20000000)");
    client.write_all(b"x")?;
    print!("  data arrives, but OUTPUT was already met so nothing was armed, want NO PULSE → ");
    print_pulse(recv_pulse(chid, 500)?.as_ref());
    unregister(&ev)?;

    Ok(())
}
