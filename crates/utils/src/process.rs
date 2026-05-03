use command_group::AsyncGroupChild;
#[cfg(unix)]
use nix::{
    errno::Errno,
    sys::signal::{Signal, kill, killpg},
    unistd::{Pid, getpgid},
};
use serde::{Deserialize, Serialize};
#[cfg(unix)]
use tokio::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub os_pid: u32,
    pub process_group_id: Option<u32>,
    pub command_snapshot: Option<String>,
    pub argv_snapshot: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessRecoveryCheck {
    AliveMatched,
    MissingPid,
    Dead,
    CommandMismatch {
        expected: String,
        actual: Option<String>,
    },
}

pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        match kill(Pid::from_raw(pid as i32), None) {
            Ok(()) => true,
            Err(Errno::EPERM) => true,
            Err(Errno::ESRCH) => false,
            Err(_) => false,
        }
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

pub fn process_group_id(pid: u32) -> Option<u32> {
    #[cfg(unix)]
    {
        getpgid(Some(Pid::from_raw(pid as i32)))
            .ok()
            .and_then(|pgid| u32::try_from(pgid.as_raw()).ok())
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

pub fn command_line_for_pid(pid: u32) -> std::io::Result<Option<String>> {
    #[cfg(unix)]
    {
        let output = std::process::Command::new("ps")
            .args(["-ww", "-p", &pid.to_string(), "-o", "command="])
            .output()?;

        if !output.status.success() {
            return Ok(None);
        }

        let command = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if command.is_empty() {
            Ok(None)
        } else {
            Ok(Some(command))
        }
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        Ok(None)
    }
}

pub fn capture_process_identity(pid: Option<u32>) -> Option<ProcessIdentity> {
    let os_pid = pid?;
    let command_snapshot = command_line_for_pid(os_pid).ok().flatten();
    let argv_snapshot = command_snapshot
        .as_deref()
        .and_then(shlex::split)
        .filter(|argv| !argv.is_empty());

    Some(ProcessIdentity {
        os_pid,
        process_group_id: process_group_id(os_pid),
        command_snapshot,
        argv_snapshot,
    })
}

pub fn command_matches_snapshot(
    expected_snapshot: Option<&str>,
    actual_command: Option<&str>,
) -> bool {
    let Some(expected) = expected_snapshot.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };

    actual_command.map(str::trim) == Some(expected)
}

pub fn check_process_for_recovery(
    os_pid: Option<i64>,
    command_snapshot: Option<&str>,
) -> ProcessRecoveryCheck {
    let Some(os_pid) = os_pid.and_then(|pid| u32::try_from(pid).ok()) else {
        return ProcessRecoveryCheck::MissingPid;
    };

    if !is_process_alive(os_pid) {
        return ProcessRecoveryCheck::Dead;
    }

    let actual_command = command_line_for_pid(os_pid).ok().flatten();
    if command_matches_snapshot(command_snapshot, actual_command.as_deref()) {
        ProcessRecoveryCheck::AliveMatched
    } else {
        ProcessRecoveryCheck::CommandMismatch {
            expected: command_snapshot.unwrap_or_default().to_string(),
            actual: actual_command,
        }
    }
}

pub async fn kill_process_group(child: &mut AsyncGroupChild) -> std::io::Result<()> {
    // hit the whole process group, not just the leader
    #[cfg(unix)]
    {
        if let Some(pid) = child.inner().id() {
            let pgid = getpgid(Some(Pid::from_raw(pid as i32)))
                .map_err(|e| std::io::Error::other(e.to_string()))?;

            for sig in [Signal::SIGINT, Signal::SIGTERM, Signal::SIGKILL] {
                tracing::info!("Sending {:?} to process group {}", sig, pgid);
                if let Err(e) = killpg(pgid, sig) {
                    tracing::warn!(
                        "Failed to send signal {:?} to process group {}: {}",
                        sig,
                        pgid,
                        e
                    );
                }
                tracing::info!("Waiting 2s for process group {} to exit", pgid);
                tokio::time::sleep(Duration::from_secs(2)).await;
                if child.inner().try_wait()?.is_some() {
                    tracing::info!("Process group {} exited after {:?}", pgid, sig);
                    break;
                }
            }
        }
    }

    let _ = child.kill().await;
    let _ = child.wait().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_pid_cannot_be_recovered() {
        assert_eq!(
            check_process_for_recovery(None, None),
            ProcessRecoveryCheck::MissingPid
        );
    }

    #[test]
    fn current_process_is_alive_and_matches_without_snapshot() {
        assert_eq!(
            check_process_for_recovery(Some(std::process::id() as i64), None),
            ProcessRecoveryCheck::AliveMatched
        );
    }

    #[test]
    fn command_snapshot_mismatch_is_detected_for_alive_process() {
        assert!(matches!(
            check_process_for_recovery(
                Some(std::process::id() as i64),
                Some("definitely-not-this-command")
            ),
            ProcessRecoveryCheck::CommandMismatch { .. }
        ));
    }

    #[test]
    fn command_snapshot_matching_trims_whitespace() {
        assert!(command_matches_snapshot(
            Some("  cargo test  "),
            Some("cargo test")
        ));
        assert!(!command_matches_snapshot(
            Some("cargo test"),
            Some("cargo check")
        ));
    }
}
