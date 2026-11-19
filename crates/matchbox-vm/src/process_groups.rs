//! Owns opt-in Unix subprocess groups for bounded execution and native host shutdown.
use std::process::{Child, Command, ExitStatus};
#[cfg(unix)]
use std::{
    collections::HashSet,
    sync::{LazyLock, Mutex},
};

/// Active groups and the native host's irreversible shutdown admission gate.
#[cfg(unix)]
static GROUPS: LazyLock<Mutex<Groups>> = LazyLock::new(|| Mutex::new(Groups::default()));

/// Registry state protected across spawning, registration and shutdown.
#[cfg(unix)]
#[derive(Default)]
struct Groups {
    /// Group IDs are the leaders' PIDs, never arbitrary caller-supplied IDs.
    ids: HashSet<u32>,
    /// Reject new owned children once host shutdown begins.
    stopping: bool,
}

/// Keeps an opted-in group owned until the surrounding execution completes or fails.
pub(crate) struct ProcessGroup {
    /// Leader PID, also the process group ID on Unix.
    pid: u32,
}

impl ProcessGroup {
    /// Spawns a child, optionally creating and registering a private Unix process group.
    ///
    /// # Errors
    /// Returns an error for unsupported platforms, shutdown, poisoned registry or spawn failure.
    pub(crate) fn spawn(
        command: &mut Command,
        owned: bool,
    ) -> Result<(Child, Option<Self>), String> {
        if !owned {
            return command
                .spawn()
                .map(|child| (child, None))
                .map_err(|e| e.to_string());
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let mut groups = GROUPS
                .lock()
                .map_err(|_| "Process group registry unavailable")?;
            if groups.stopping {
                return Err("Native host is stopping".into());
            }
            command.process_group(0);
            let child = command.spawn().map_err(|e| e.to_string())?;
            let pid = child.id();
            groups.ids.insert(pid);
            Ok((child, Some(Self { pid })))
        }
        #[cfg(not(unix))]
        Err("systemExecute() terminateProcessGroup is supported only on Unix".into())
    }

    /// Observes leader exit without reaping it, stops descendants, then reaps the leader.
    ///
    /// Keeping the leader unreaped until the group is stopped prevents a recycled PID from
    /// making cleanup signal an unrelated group. Returns `None` while the leader is running.
    ///
    /// # Errors
    /// Returns registry, wait or signal failures. Unexpected external reaping relinquishes
    /// ownership rather than signalling a process whose identity is no longer known.
    pub(crate) fn try_wait(&self, child: &mut Child) -> Result<Option<ExitStatus>, String> {
        #[cfg(unix)]
        {
            let mut groups = GROUPS
                .lock()
                .map_err(|_| "Process group registry unavailable")?;
            if !groups.ids.contains(&self.pid) {
                return child.try_wait().map_err(|e| e.to_string());
            }
            loop {
                // waitid writes a siginfo_t; zero its no-event result as required by POSIX.
                let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
                let result = unsafe {
                    libc::waitid(
                        libc::P_PID,
                        self.pid as libc::id_t,
                        &mut info,
                        libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                    )
                };
                if result != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    if error.raw_os_error() == Some(libc::ECHILD) {
                        groups.ids.remove(&self.pid);
                    }
                    return Err(error.to_string());
                }
                if unsafe { info.si_pid() } == 0 {
                    return Ok(None);
                }
                kill_group(self.pid).map_err(|e| e.to_string())?;
                groups.ids.remove(&self.pid);
                return child.wait().map(Some).map_err(|e| e.to_string());
            }
        }
        #[cfg(not(unix))]
        child.try_wait().map_err(|e| e.to_string())
    }

    /// Kills and unregisters this owned group before the caller reaps its leader.
    ///
    /// # Errors
    /// Returns registry or signal failures; successful termination is never repeated.
    pub(crate) fn terminate(&self) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            let mut groups = GROUPS
                .lock()
                .map_err(|_| std::io::Error::other("Process group registry unavailable"))?;
            if groups.ids.contains(&self.pid) {
                kill_group(self.pid)?;
                groups.ids.remove(&self.pid);
            }
        }
        Ok(())
    }
}

impl Drop for ProcessGroup {
    /// Stops remaining descendants on every return path and removes the group from the registry.
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Ok(mut groups) = GROUPS.lock() {
            if groups.ids.remove(&self.pid) && kill_group(self.pid).is_ok() {
                // Error/unwind cleanup owns an unreaped leader. Avoid leaking a zombie.
                while unsafe { libc::waitpid(self.pid as libc::pid_t, std::ptr::null_mut(), 0) } < 0
                {
                    if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                        break;
                    }
                }
            }
        }
    }
}

/// Sends SIGKILL only to a process group created by this module.
///
/// # Errors
/// Returns the OS failure unless the group no longer exists.
#[cfg(unix)]
fn kill_group(pid: u32) -> std::io::Result<()> {
    // A spawned OS PID is positive and fits pid_t on supported Unix targets.
    if unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

/// Stops admission of owned process groups and kills active ones before native server drain.
///
/// This affects only `systemExecute(..., terminateProcessGroup=true)`. It does not stop
/// unrelated processes, undo remote side effects or contain descendants which escape their
/// group. Shutdown is irreversible for this process. Abrupt host termination cannot run it.
///
/// # Errors
/// Returns a registry or OS error. All registered groups are attempted before reporting failure.
pub fn terminate_owned_process_groups() -> Result<(), String> {
    #[cfg(unix)]
    {
        let mut groups = GROUPS
            .lock()
            .map_err(|_| "Process group registry unavailable")?;
        groups.stopping = true;
        let mut failure = None;
        for pid in std::mem::take(&mut groups.ids) {
            if let Err(error) = kill_group(pid) {
                groups.ids.insert(pid);
                failure = Some(error.to_string());
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
    }
    Ok(())
}
