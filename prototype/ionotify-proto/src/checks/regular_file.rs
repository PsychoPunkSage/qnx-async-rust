//! What `ionotify()` does on a regular file. A resource manager without notify
//! support answers ENOSYS; QNX's own `poll()` then reports the fd as ready, and
//! POSIX says regular files always poll ready.

use std::{fs::OpenOptions, io, os::fd::AsRawFd};

use crate::{
    ffi,
    utils::{check, print_pulse, recv_pulse, READY},
};

pub fn run(chid: i32, coid: i32) -> io::Result<()> {
    println!("[regular_file] ionotify on a plain file");
    let path = std::env::temp_dir().join(format!("ionotify-proto.{}", std::process::id()));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)?;
    let fd = file.as_raw_fd();

    let mut ev = ffi::SigEvent::pulse(coid, 10, READY, 0);
    // SAFETY: `ev` has the C sigevent layout; the kernel may rewrite it.
    let reg = unsafe { libc::MsgRegisterEvent((&mut ev as *mut ffi::SigEvent).cast(), fd) };
    if reg == -1 {
        println!("  MsgRegisterEvent: failed: {}", io::Error::last_os_error());
    } else {
        println!("  MsgRegisterEvent: ok");
    }

    let conds = ffi::NOTIFY_COND_INPUT | ffi::NOTIFY_COND_OUTPUT;
    // SAFETY: `fd` is open, `ev` outlives the call.
    let ret = unsafe { ffi::ionotify(fd, ffi::NOTIFY_ACTION_POLLARM, conds, &ev) };
    if ret == -1 {
        let e = io::Error::last_os_error();
        let tag = if e.raw_os_error() == Some(libc::ENOSYS) {
            " (ENOSYS)"
        } else {
            ""
        };
        println!("  POLLARM INPUT|OUTPUT: -1, {e}{tag}");
    } else {
        println!("  POLLARM INPUT|OUTPUT: {ret:#x}");
    }
    print!("  any pulse? → ");
    print_pulse(recv_pulse(chid, 500)?.as_ref());

    if reg != -1 {
        // SAFETY: `ev` was registered above.
        check(
            unsafe { libc::MsgUnregisterEvent((&ev as *const ffi::SigEvent).cast()) },
            "MsgUnregisterEvent",
        )?;
    }
    drop(file);
    std::fs::remove_file(&path)?;
    Ok(())
}
