//! A pulse sent to our own channel arrives with the code and value we sent.
//! This is the Waker path: one `MsgSendPulse()`, no pipe and no extra fd.

use std::io;

use crate::utils::{check, print_pulse, recv_pulse, WAKE};

pub fn run(chid: i32, coid: i32) -> io::Result<()> {
    println!("[waker] self pulse");
    // Priority 10, code WAKE, value 42.
    // SAFETY: plain syscall, no pointers.
    check(
        unsafe { libc::MsgSendPulse(coid, 10, WAKE as libc::c_int, 42) },
        "MsgSendPulse",
    )?;
    print!("  want code = 1, value = 0x2a → ");
    print_pulse(recv_pulse(chid, 1000)?.as_ref());

    Ok(())
}
