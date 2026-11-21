//! Helpers shared by the checks: error mapping, receiving a pulse with a
//! timeout, and printing a pulse.

use std::{io, mem::MaybeUninit, ptr};

use crate::ffi;

/// Pulse code for a wake-up sent by ourselves (the Waker path).
pub const WAKE: i8 = 1;
/// Pulse code for "a watched fd is ready".
pub const READY: i8 = 2;

/// Turn a C-style `-1` return into an `io::Error`.
pub fn check(ret: i32, what: &str) -> io::Result<i32> {
    if ret == -1 {
        let e = io::Error::last_os_error();
        eprintln!("{what} failed: {e}");
        Err(e)
    } else {
        Ok(ret)
    }
}

/// Wait up to `ms` milliseconds for one pulse on `chid`. `Ok(None)` means timed out.
pub fn recv_pulse(chid: i32, ms: u64) -> io::Result<Option<ffi::Pulse>> {
    let ns = ms * 1_000_000;
    // SAFETY: arms a one-shot timeout for the next receive-blocking call on this
    // thread; NULL event = just unblock. `ns` outlives the call.
    check(
        unsafe {
            libc::TimerTimeout(
                libc::CLOCK_MONOTONIC,
                libc::_NTO_TIMEOUT_RECEIVE,
                ptr::null(),
                &ns,
                ptr::null_mut(),
            )
        },
        "TimerTimeout",
    )?;

    // Blocks until a pulse arrives or the timeout above fires.
    let mut pulse = MaybeUninit::<ffi::Pulse>::uninit();
    // SAFETY: the buffer is a valid, writable ffi::Pulse of the size we pass.
    let ret = unsafe {
        libc::MsgReceivePulse(
            chid,
            pulse.as_mut_ptr().cast(),
            size_of::<ffi::Pulse>(),
            ptr::null_mut(),
        )
    };

    if ret == -1 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::ETIMEDOUT) {
            return Ok(None);
        }
        return Err(e);
    }
    // SAFETY: MsgReceivePulse returned success, so it filled the buffer.
    Ok(Some(unsafe { pulse.assume_init() }))
}

/// Print a received pulse's code and raw value, or that none arrived.
pub fn print_pulse(p: Option<&ffi::Pulse>) {
    match p {
        Some(p) => {
            // SAFETY: every member of sigval is plain data; reading it as a pointer-sized
            // integer shows all bits whichever member the sender wrote.
            let raw = unsafe { p.value.sival_ptr } as usize;
            println!("pulse: code = {}, value = {raw:#x}", p.code);
        }
        None => println!("NO PULSE (timeout)"),
    }
}
