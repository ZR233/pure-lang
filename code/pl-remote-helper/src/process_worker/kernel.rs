use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Command, ExitStatus};

/// Only constructed by the single-threaded worker entrypoint, before spawning its command.
pub(super) struct Kernel {
    signals: OwnedFd,
    inherited_mask: libc::sigset_t,
    inherited_child_action: libc::sigaction,
}

impl Kernel {
    pub(super) fn new() -> io::Result<Self> {
        // SAFETY: these C signal-mask/action structures permit an all-zero initialization.
        // All pointers below address initialized, exclusively borrowed local storage.
        let (mut mask, mut inherited_mask, mut action, mut inherited_child_action): (
            libc::sigset_t,
            libc::sigset_t,
            libc::sigaction,
            libc::sigaction,
        ) = unsafe { std::mem::zeroed() };
        // SAFETY: the worker is single-threaded and has no child or registered Rust signal handler.
        // SIGCHLD must remain waitable even if its disposition was inherited as ignored.
        unsafe {
            if libc::sigemptyset(&mut mask) != 0 {
                return Err(io::Error::last_os_error());
            }
            for signal in [libc::SIGCHLD, libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
                if libc::sigaddset(&mut mask, signal) != 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            let result = libc::pthread_sigmask(libc::SIG_BLOCK, &mask, &mut inherited_mask);
            if result != 0 {
                return Err(io::Error::from_raw_os_error(result));
            }
            action.sa_sigaction = libc::SIG_DFL;
            if libc::sigaction(libc::SIGCHLD, &action, &mut inherited_child_action) != 0
                || libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) != 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        // SAFETY: mask is initialized and remains borrowed for this call. Successful signalfd
        // returns a new descriptor, transferred exactly once into OwnedFd.
        let descriptor =
            unsafe { libc::signalfd(-1, &mask, libc::SFD_CLOEXEC | libc::SFD_NONBLOCK) };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: descriptor is a newly returned, nonnegative fd with no other Rust owner.
        let signals = unsafe { OwnedFd::from_raw_fd(descriptor) };
        let probe = pidfd(std::process::id() as libc::pid_t)?;
        signal(&probe, 0)?;
        children()?;
        Ok(Self {
            signals,
            inherited_mask,
            inherited_child_action,
        })
    }

    pub(super) fn prepare_command(&self, command: &mut Command) {
        let mask = self.inherited_mask;
        let action = self.inherited_child_action;
        // SAFETY: after fork this closure only restores copied signal state with async-signal-safe
        // libc calls. It allocates nothing and takes no locks. Neither the worker's signal mask
        // nor its SIGCHLD disposition may leak into the executed business program.
        unsafe {
            command.pre_exec(move || {
                if libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) != 0 {
                    return Err(io::Error::last_os_error());
                }
                let result = libc::pthread_sigmask(libc::SIG_SETMASK, &mask, std::ptr::null_mut());
                if result == 0 {
                    Ok(())
                } else {
                    Err(io::Error::from_raw_os_error(result))
                }
            });
        }
    }

    pub(super) fn wait(&self, control: Option<(RawFd, i16)>, timeout: i32) -> io::Result<()> {
        let mut fds = [
            libc::pollfd {
                fd: self.signals.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: control.map_or(-1, |(fd, _)| fd),
                events: control.map_or(0, |(_, events)| events),
                revents: 0,
            },
        ];
        // SAFETY: fds is a live mutable two-element pollfd array; descriptors remain owned.
        let result = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        if fds.iter().any(|fd| fd.revents & libc::POLLNVAL != 0) {
            return Err(io::Error::from_raw_os_error(libc::EBADF));
        }
        Ok(())
    }

    pub(super) fn cancellation_signal(&self) -> io::Result<bool> {
        let mut cancelled = false;
        loop {
            // SAFETY: signalfd_siginfo consists of scalar integers; zero initializes valid storage.
            let mut info: libc::signalfd_siginfo = unsafe { std::mem::zeroed() };
            // SAFETY: the fd remains owned, and info is aligned writable storage of the given size.
            let count = unsafe {
                libc::read(
                    self.signals.as_raw_fd(),
                    (&mut info as *mut libc::signalfd_siginfo).cast(),
                    std::mem::size_of_val(&info),
                )
            };
            if count < 0 {
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::WouldBlock => return Ok(cancelled),
                    io::ErrorKind::Interrupted => continue,
                    _ => return Err(error),
                }
            }
            if count as usize != std::mem::size_of_val(&info) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "incomplete signalfd record",
                ));
            }
            cancelled |= info.ssi_signo != libc::SIGCHLD as u32;
        }
    }
}

pub(super) fn children() -> io::Result<Vec<libc::pid_t>> {
    let pid = std::process::id();
    std::fs::read_to_string(format!("/proc/self/task/{pid}/children"))?
        .split_whitespace()
        .map(|pid| {
            pid.parse::<libc::pid_t>()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        })
        .collect()
}

/// Called only for direct unreaped children (or the initial self probe). This worker is the
/// exclusive reaper, so a PID read from its own children list cannot be reused before pidfd_open.
pub(super) fn pidfd(pid: libc::pid_t) -> io::Result<OwnedFd> {
    // SAFETY: syscall arguments follow pidfd_open(pid_t, unsigned int); no pointer is supplied.
    let descriptor = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0_u32) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Linux returns a new fd in the nonnegative c_int range, with CLOEXEC set.
    Ok(unsafe { OwnedFd::from_raw_fd(descriptor as RawFd) })
}

pub(super) fn signal(fd: &OwnedFd, signal: i32) -> io::Result<()> {
    // SAFETY: fd remains borrowed and owned for the call. A null siginfo requests a normal
    // signal; the kernel validates the signal and descriptor without interpreting user memory.
    let result = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            signal,
            std::ptr::null::<libc::siginfo_t>(),
            0_u32,
        )
    };
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(super) enum Reaped {
    Child(libc::pid_t, ExitStatus),
    StateChanged,
    Pending,
    Empty,
}

pub(super) fn reap() -> io::Result<Reaped> {
    let mut status = 0;
    // SAFETY: this single-threaded process owns and exclusively waits for all of its children.
    // status is an initialized, writable c_int; WNOHANG never blocks the control loop.
    let pid = unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG | libc::__WALL) };
    if pid > 0 {
        let status = ExitStatus::from_raw(status);
        if status.code().is_some() || status.signal().is_some() {
            Ok(Reaped::Child(pid, status))
        } else {
            // A ptrace stop is observable even without WUNTRACED. It does not reap the child
            // and must not release its pidfd or become the command's terminal result.
            Ok(Reaped::StateChanged)
        }
    } else if pid == 0 {
        Ok(Reaped::Pending)
    } else {
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ECHILD) {
            Ok(Reaped::Empty)
        } else {
            Err(error)
        }
    }
}

pub(super) fn close_on_exec(fd: RawFd) -> io::Result<()> {
    // SAFETY: fd is borrowed from the worker's live control stream; flags are scalar values.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}
