//! QNX 8 items missing from the libc crate.
//! Headers: <SDP>/target/qnx/usr/include/

use std::os::raw::{c_int, c_long, c_uint, c_ulong, c_void};

// ---- sys/iomgr.h  conditions (the "what") ----
pub const NOTIFY_COND_OBAND: c_int = 0x40000000;
pub const NOTIFY_COND_OUTPUT: c_int = 0x20000000;
pub const NOTIFY_COND_INPUT: c_int = 0x10000000;
/// Set in the flags to use the extended conditions below (low bits).
pub const NOTIFY_COND_EXTEN: c_int = 0x80000000_u32 as c_int;

// ---- sys/iomgr.h  extended conditions (only with NOTIFY_COND_EXTEN) ----
pub const NOTIFY_CONDE_RDNORM: c_int = 0x00000001;
pub const NOTIFY_CONDE_WRNORM: c_int = 0x00000002;
pub const NOTIFY_CONDE_ERR: c_int = 0x00000020;
pub const NOTIFY_CONDE_HUP: c_int = 0x00000040;
pub const NOTIFY_CONDE_NVAL: c_int = 0x00001000;

// ---- sys/iomgr.h  actions (the "how") ----
pub const NOTIFY_ACTION_POLL: c_int = 0x2;
pub const NOTIFY_ACTION_POLLARM: c_int = 0x3;

// ---- sys/siginfo.h ----
pub const SIGEV_PULSE: c_int = 4;

// ---- sys/resource.h ----
pub const RLIMIT_SIGEVENT_NP: c_int = 12;

// ---- sys/siginfo.h  union sigval ----
#[repr(C)]
pub union SigVal {
    pub sival_int: c_int,
    pub sival_ptr: *mut c_void,
    pub sival_long: c_long,
    pub sival_ulong: c_ulong,
}

// ---- sys/siginfo.h  struct sigevent: union __sigev_un1 ----
#[repr(C)]
pub union SigevUn1 {
    pub sigev_signo: c_int,
    pub sigev_coid: c_int,
    pub sigev_id: c_int,
    pub sigev_notify_function: Option<unsafe extern "C" fn(SigVal)>,
    pub sigev_addr: *mut c_uint, // volatile unsigned *
    pub sigev_handle: c_uint,
}

// ---- sys/siginfo.h  struct sigevent: __sigev_un2.__st ----
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SigevSt {
    pub sigev_code: i16,     // short
    pub sigev_priority: i16, // short
}

// ---- sys/siginfo.h  struct sigevent: union __sigev_un2 ----
#[repr(C)]
pub union SigevUn2 {
    pub st: SigevSt,
    pub sigev_notify_attributes: *mut libc::pthread_attr_t,
    pub sigev_memop: c_int,
}

// ---- sys/siginfo.h  struct sigevent ----
#[repr(C)]
pub struct SigEvent {
    pub sigev_notify: c_int,
    pub sigev_un1: SigevUn1,
    pub sigev_value: SigVal,
    pub sigev_un2: SigevUn2,
}

impl SigEvent {
    /// Same as the C macro SIGEV_PULSE_INIT (sys/siginfo.h).
    pub fn pulse(coid: c_int, priority: i16, code: i8, value: usize) -> Self {
        SigEvent {
            sigev_notify: SIGEV_PULSE,
            sigev_un1: SigevUn1 { sigev_coid: coid },
            sigev_value: SigVal {
                sival_ptr: value as *mut c_void,
            },
            sigev_un2: SigevUn2 {
                st: SigevSt {
                    sigev_code: code as i16,
                    sigev_priority: priority,
                },
            },
        }
    }
}

// ---- sys/neutrino.h  struct _pulse ----
#[repr(C)]
pub struct Pulse {
    pub r#type: u16,
    pub subtype: u16,
    pub code: i8,
    pub zero: [u8; 3],
    pub value: SigVal,
    pub scoid: i32,
}

// ---- unistd.h ----
unsafe extern "C" {
    pub fn ionotify(fd: c_int, action: c_int, flags: c_int, event: *const SigEvent) -> c_int;
}
