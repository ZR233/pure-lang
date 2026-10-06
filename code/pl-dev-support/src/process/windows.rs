//! Windows kill-on-close Job Object backing `own_current_process_tree`.

use anyhow::{Context, Result, bail};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

static RESIDENT_PROCESS_JOB: OnceLock<Result<ResidentProcessJob, String>> = OnceLock::new();

/// See `own_current_process_tree` in the parent module.
///
/// The result (success or failure) is cached for the process lifetime, so a
/// broken Job Object is reported once instead of being retried per child.
pub(super) fn own_current_process_tree() -> Result<()> {
    match RESIDENT_PROCESS_JOB
        .get_or_init(|| ResidentProcessJob::create().map_err(|error| format!("{error:#}")))
    {
        Ok(_) => Ok(()),
        Err(error) => bail!("{error}"),
    }
}

/// The owned Job Object handle.
///
/// The handle is created null-checked, assigned to the current process, and
/// closed exactly once from [`Drop`]. The `OnceLock` above keeps the value
/// alive until process exit, so no other code can close or reuse the handle
/// and the unsafe calls below always receive a live, non-null `HANDLE`.
struct ResidentProcessJob {
    handle: isize,
}

impl ResidentProcessJob {
    fn create() -> Result<Self> {
        // SAFETY: `CreateJobObjectW` reads no memory from our arguments (both
        // pointers are null to request default name and attributes) and either
        // returns a fresh owned handle or null with the error in
        // `GetLastError`, which is checked immediately below.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error()).context("create Windows Job Object");
        }

        let result = configure_kill_on_close(handle)
            .and_then(|()| assign_current_process(handle))
            .map(|()| Self {
                handle: handle as isize,
            });
        if result.is_err() {
            // SAFETY: the handle is still owned by this function (nothing has
            // stored or closed it yet), so it is valid to close here once.
            unsafe {
                CloseHandle(handle);
            }
        }
        result
    }
}

fn configure_kill_on_close(handle: HANDLE) -> Result<()> {
    // SAFETY: `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` is a plain C struct of
    // integrals where an all-zero value is valid; zeroing it only leaves every
    // limit unset before `LimitFlags` is assigned below.
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    // Only explicit installer/replacement handoffs may leave the job. Ordinary
    // children remain owned and are still killed when the resident command exits.
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    // SAFETY: the pointer names our local `limits` struct for the duration of
    // the call, the size matches the declared information class, and `handle`
    // is the live Job Object handle created by `ResidentProcessJob::create`.
    let configured = unsafe {
        SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            std::ptr::addr_of_mut!(limits).cast::<c_void>(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured == 0 {
        Err(std::io::Error::last_os_error()).context("configure Windows Job Object")
    } else {
        Ok(())
    }
}

fn assign_current_process(handle: HANDLE) -> Result<()> {
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that is valid for
    // the whole lifetime of this process and needs no closing, and `handle`
    // is the live Job Object handle created by `ResidentProcessJob::create`.
    let assigned = unsafe { AssignProcessToJobObject(handle, GetCurrentProcess()) };
    if assigned == 0 {
        Err(std::io::Error::last_os_error()).context("assign xtask to Windows Job Object")
    } else {
        Ok(())
    }
}

impl Drop for ResidentProcessJob {
    fn drop(&mut self) {
        // SAFETY: the stored handle was created non-null, has never been
        // closed elsewhere, and `Drop` runs at most once, so closing it here
        // releases the process's last reference (the kill-on-close behavior
        // then terminates every child that joined the job).
        unsafe {
            CloseHandle(self.handle as HANDLE);
        }
    }
}
