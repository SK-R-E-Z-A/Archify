use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_LIMIT_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Threading::{
    CreateProcessW, PROCESS_CREATION_FLAGS, PROCESS_INFORMATION, STARTUPINFOW,
};
use windows::core::{PCWSTR, PWSTR};

/// Error type for Windows Job Object operations.
#[derive(Debug)]
pub enum JobError {
    CreateJobObject(String),
    SetJobInfo(String),
    AssignProcess(String),
    TerminateJob(String),
    ProcessSpawn(String),
}

impl std::fmt::Display for JobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobError::CreateJobObject(msg) => write!(f, "Failed to create Job Object: {}", msg),
            JobError::SetJobInfo(msg) => write!(f, "Failed to set Job Object information: {}", msg),
            JobError::AssignProcess(msg) => write!(f, "Failed to assign process to Job: {}", msg),
            JobError::TerminateJob(msg) => write!(f, "Failed to terminate Job: {}", msg),
            JobError::ProcessSpawn(msg) => write!(f, "Failed to spawn process: {}", msg),
        }
    }
}

impl std::error::Error for JobError {}

/// Represents a Windows Job Object for process-tree management.
#[derive(Debug)]
pub struct WindowsJob {
    job_handle: HANDLE,
}

impl WindowsJob {
    /// Create a new Job Object with the appropriate limits for process-tree management.
    pub fn new() -> Result<Self, JobError> {
        let job_handle = unsafe { CreateJobObjectW(None, None) }
            .map_err(|e| JobError::CreateJobObject(format!("{:?}", e)))?;

        // Configure job limits
        let basic_limit = JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
            ..Default::default()
        };

        let extended_limit = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
            BasicLimitInformation: basic_limit,
            ..Default::default()
        };

        unsafe {
            SetInformationJobObject(
                job_handle,
                JobObjectExtendedLimitInformation,
                &extended_limit as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| JobError::SetJobInfo(format!("{:?}", e)))?;
        }

        Ok(Self { job_handle })
    }

    pub fn assign_process(&self, process_handle: HANDLE) -> Result<(), JobError> {
        unsafe {
            AssignProcessToJobObject(self.job_handle, process_handle)
                .map_err(|e| JobError::AssignProcess(format!("{:?}", e)))
        }
    }

    pub fn terminate(&self) -> Result<(), JobError> {
        unsafe {
            TerminateJobObject(self.job_handle, 1)
                .map_err(|e| JobError::TerminateJob(format!("{:?}", e)))?;
        }
        Ok(())
    }
}

impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.job_handle);
        }
    }
}

// A Job Object handle is a kernel handle that can safely be moved and shared
// between threads. The struct owns it uniquely and only closes it on drop.
unsafe impl Send for WindowsJob {}
unsafe impl Sync for WindowsJob {}

/// Represents a process that is managed by a Job Object.
#[derive(Debug)]
pub struct ManagedProcess {
    // Kept alive for the lifetime of the process: dropping the Job Object
    // triggers JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, terminating the tree.
    #[allow(dead_code)]
    job: WindowsJob,
    pub process_info: PROCESS_INFORMATION,
}

impl ManagedProcess {
    /// Spawn a new process and assign it to a Job Object.
    pub fn spawn(
        command: &str,
        args: &[&str],
        working_dir: &std::path::Path,
    ) -> Result<Self, JobError> {
        let job = WindowsJob::new()?;

        let mut command_line: Vec<u16> = format!("{} {}", command, args.join(" "))
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let working_dir_w: Vec<u16> = working_dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut startup_info: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup_info.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        let mut process_info = unsafe { std::mem::zeroed() };

        unsafe {
            if CreateProcessW(
                PCWSTR::null(),
                Some(PWSTR(command_line.as_mut_ptr())),
                None,
                None,
                false,
                PROCESS_CREATION_FLAGS(0),
                None,
                PCWSTR(working_dir_w.as_ptr()),
                &startup_info,
                &mut process_info,
            )
            .is_err()
            {
                return Err(JobError::ProcessSpawn(
                    "Failed to create process".to_string(),
                ));
            }

            // Assign the just-created process to the job immediately
            if let Err(e) = job.assign_process(process_info.hProcess) {
                // Cleanup on failure
                let _ = CloseHandle(process_info.hProcess);
                let _ = CloseHandle(process_info.hThread);
                return Err(e);
            }
        }

        Ok(Self { job, process_info })
    }

    /// Terminate the process immediately.
    pub fn terminate(&mut self) -> Result<(), JobError> {
        unsafe {
            windows::Win32::System::Threading::TerminateProcess(self.process_info.hProcess, 1)
                .map_err(|e| JobError::TerminateJob(format!("{:?}", e)))?;

            windows::Win32::System::Threading::WaitForSingleObject(
                self.process_info.hProcess,
                windows::Win32::System::Threading::INFINITE,
            );
        }
        Ok(())
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.process_info.hProcess);
            let _ = CloseHandle(self.process_info.hThread);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::STILL_ACTIVE;
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};

    #[test]
    fn test_job_object_termination() {
        let working_dir = std::env::current_dir().unwrap();

        // Use a process that will stay alive
        // "cmd /c timeout /t 60" is common on Windows
        let process =
            ManagedProcess::spawn("cmd.exe", &["/c", "timeout", "/t", "60"], &working_dir)
                .expect("Failed to spawn process");

        let h_process = process.process_info.hProcess;

        // Verify it is active
        let mut exit_code: u32 = 0;
        unsafe {
            GetExitCodeProcess(h_process, &mut exit_code).expect("Failed to get exit code");
            assert_eq!(exit_code as i32, STILL_ACTIVE.0, "Process should be active");
        }

        // Drop the ManagedProcess, which should kill the process via Job Object
        drop(process);

        // We closed the handle in Drop, so we can't use h_process anymore safely
        // if we want to be strictly correct, but for the test we want to prove it's dead.
        // Actually, CloseHandle doesn't invalidate the value, just the handle.
        // BUT we need a handle to check if it's dead.

        // Let's re-run the logic but keep the handle open separately.
    }

    #[test]
    fn test_managed_process_termination_on_drop() {
        let working_dir = std::env::current_dir().unwrap();

        // 1. Create Job and spawn process
        let process =
            ManagedProcess::spawn("cmd.exe", &["/c", "timeout", "/t", "60"], &working_dir)
                .expect("Failed to spawn process");

        let h_process = process.process_info.hProcess;

        // 2. Duplicate the handle so we can check it after ManagedProcess is dropped
        let mut h_dup = HANDLE::default();
        unsafe {
            use windows::Win32::Foundation::DUPLICATE_SAME_ACCESS;
            use windows::Win32::System::Threading::GetCurrentProcess;
            windows::Win32::Foundation::DuplicateHandle(
                GetCurrentProcess(),
                h_process,
                GetCurrentProcess(),
                &mut h_dup,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
            .expect("Failed to duplicate handle");
        }

        // 3. Verify it is active
        let mut exit_code: u32 = 0;
        unsafe {
            GetExitCodeProcess(h_dup, &mut exit_code).expect("Failed to get exit code");
            assert_eq!(exit_code as i32, STILL_ACTIVE.0, "Process should be active");
        }

        // 4. Drop the ManagedProcess. This closes the Job handle.
        // Because of JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, the process should die.
        drop(process);

        // 5. Wait for the process to terminate (it should be immediate)
        unsafe {
            let wait_res = WaitForSingleObject(h_dup, 5000); // Wait up to 5s
            assert_eq!(
                wait_res,
                windows::Win32::Foundation::WAIT_OBJECT_0,
                "Process should have terminated"
            );

            GetExitCodeProcess(h_dup, &mut exit_code).expect("Failed to get exit code after drop");
            assert_ne!(
                exit_code as i32, STILL_ACTIVE.0,
                "Process should not be active anymore"
            );

            let _ = CloseHandle(h_dup);
        }
    }
}
