//! The hand-written FFI structs have the same size as their C counterparts
//! (`struct _pulse` = 24 bytes, `struct sigevent` = 32 bytes on 64-bit).

use std::mem::size_of;

use crate::ffi;

pub fn run() {
    println!("[layout] FFI struct sizes");
    println!("  Pulse = {} (want 24)", size_of::<ffi::Pulse>());
    println!("  SigEvent = {} (want 32)", size_of::<ffi::SigEvent>());
    println!(
        "  libc::sigevent = {} (want 32)",
        size_of::<libc::sigevent>()
    );
}
