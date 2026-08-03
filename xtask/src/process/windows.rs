use anyhow::{Context, Result, bail};
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::OnceLock;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

static RESIDENT_PROCESS_JOB: OnceLock<Result<ResidentProcessJob, String>> = OnceLock::new();

pub(super) fn own_current_process_tree() -> Result<()> {
    match RESIDENT_PROCESS_JOB
        .get_or_init(|| ResidentProcessJob::create().map_err(|error| format!("{error:#}")))
    {
        Ok(_) => Ok(()),
        Err(error) => bail!("{error}"),
    }
}

struct ResidentProcessJob {
    handle: isize,
}

impl ResidentProcessJob {
    fn create() -> Result<Self> {
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
            unsafe {
                CloseHandle(handle);
            }
        }
        result
    }
}

fn configure_kill_on_close(handle: HANDLE) -> Result<()> {
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
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
    let assigned = unsafe { AssignProcessToJobObject(handle, GetCurrentProcess()) };
    if assigned == 0 {
        Err(std::io::Error::last_os_error()).context("assign xtask to Windows Job Object")
    } else {
        Ok(())
    }
}

impl Drop for ResidentProcessJob {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle as HANDLE);
        }
    }
}
