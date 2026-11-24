//! Go/no-go prototype for an `ionotify()` + pulse based mio selector on QNX 8.
//!
//! Each module in `checks` answers one question about how `ionotify()`, pulses
//! and `MsgRegisterEvent()` behave, printing what it expects next to what it got.
//! All checks share one channel (the "mailbox" pulses are delivered to) and one
//! side-channel connection to it.

mod checks;
mod ffi;
mod utils;

use std::io;

use utils::check;

fn main() -> io::Result<()> {
    // Our mailbox, and a side-channel link to it. Side-channel coids use their
    // own number range, so they never collide with fds.
    // SAFETY: plain syscalls, no pointers.
    let chid = check(
        unsafe { libc::ChannelCreate(libc::_NTO_CHF_PRIVATE) },
        "ChannelCreate",
    )?;
    // SAFETY: plain syscall, no pointers; nd = 0 and pid = 0 mean this node and process.
    let coid = check(
        unsafe { libc::ConnectAttach(0, 0, chid, libc::_NTO_SIDE_CHANNEL, 0) },
        "ConnectAttach",
    )?;

    // `cargo run -- <name>` runs one check; no argument runs them all.
    let only = std::env::args().nth(1);
    let want = |name: &str| only.as_deref().map_or(true, |only| only == name);

    if want("layout") {
        checks::layout::run();
    }
    if want("waker") {
        checks::waker::run(chid, coid)?;
    }
    if want("tcp_readable") {
        checks::tcp_readable::run(chid, coid)?;
    }
    if want("rearm") {
        checks::rearm::run(chid, coid)?;
    }
    if want("arm_race") {
        checks::arm_race::run(chid, coid)?;
    }
    if want("event_limits") {
        checks::event_limits::run(coid)?;
    }
    if want("peer_close") {
        checks::peer_close::run(chid, coid)?;
    }
    if want("regular_file") {
        checks::regular_file::run(chid, coid)?;
    }
    if want("exten") {
        checks::exten::run(chid, coid)?;
    }
    if want("hup_poll") {
        checks::hup_poll::run(chid, coid)?;
    }

    // SAFETY: coid and chid are ours and still open.
    unsafe {
        libc::ConnectDetach(coid);
        libc::ChannelDestroy(chid);
    }

    Ok(())
}
