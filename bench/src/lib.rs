//! Shared helpers for the echo benchmark: raising process limits and reading
//! a process's CPU time on QNX.

use std::fs;
use std::io;
use std::os::fd::AsRawFd;

/// From `sys/resource.h`; libc doesn't define it.
const RLIMIT_SIGEVENT_NP: libc::c_int = 12;
/// `DCMD_PROC_INFO` from `sys/procfs.h`.
const DCMD_PROC_INFO: libc::c_int = 0x4110_0801;
/// `sizeof(debug_process_t)` and the offsets of its `utime` / `stime` (ns).
const PROC_INFO_SIZE: usize = 272;
const UTIME_OFFSET: usize = 120;
const STIME_OFFSET: usize = 128;

extern "C" {
    /// `devctl.h`. Returns EOK (0) or an errno value.
    fn devctl(
        fd: libc::c_int,
        dcmd: libc::c_int,
        data: *mut libc::c_void,
        nbytes: libc::size_t,
        info: *mut libc::c_int,
    ) -> libc::c_int;
}

fn set_limit(resource: libc::c_int, value: u64) -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value as libc::rlim_t,
        rlim_max: value as libc::rlim_t,
    };
    // SAFETY: `limit` is a valid rlimit.
    if unsafe { libc::setrlimit(resource, &limit) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Raise the open-file limit, and the registered-event limit that the
/// ionotify selector uses one of per registration. Both selectors get the
/// same limits, so runs differ only in the selector. Needs root.
pub fn raise_limits(fds: u64) -> io::Result<()> {
    set_limit(libc::RLIMIT_NOFILE, fds)?;
    set_limit(RLIMIT_SIGEVENT_NP, fds)
}

/// Raise only the open-file limit (for the client).
pub fn raise_fd_limit(fds: u64) -> io::Result<()> {
    set_limit(libc::RLIMIT_NOFILE, fds)
}

/// The pid of the first process whose executable path ends in `/name`.
pub fn find_pid(name: &str) -> io::Result<libc::pid_t> {
    let suffix = format!("/{name}");
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<libc::pid_t>() else {
            continue;
        };
        let Ok(path) = fs::read(entry.path().join("exefile")) else {
            continue;
        };
        let path = String::from_utf8_lossy(&path);
        if path.trim_end_matches(['\0', ' ', '\n']).ends_with(&suffix) {
            return Ok(pid);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("no process named {name}"),
    ))
}

/// User + system CPU time of process `pid`, in nanoseconds.
pub fn cpu_time_ns(pid: libc::pid_t) -> io::Result<u64> {
    let ctl = fs::File::open(format!("/proc/{pid}/ctl"))?;
    let mut info = [0u8; PROC_INFO_SIZE];
    // SAFETY: `info` is writable for `PROC_INFO_SIZE` bytes, the size of
    // `debug_process_t`; a null `info` pointer is allowed.
    let ret = unsafe {
        devctl(
            ctl.as_raw_fd(),
            DCMD_PROC_INFO,
            info.as_mut_ptr().cast(),
            PROC_INFO_SIZE,
            std::ptr::null_mut(),
        )
    };
    if ret != 0 {
        return Err(io::Error::from_raw_os_error(ret));
    }
    let field =
        |offset: usize| u64::from_ne_bytes(info[offset..offset + 8].try_into().expect("8 bytes"));
    Ok(field(UTIME_OFFSET) + field(STIME_OFFSET))
}
