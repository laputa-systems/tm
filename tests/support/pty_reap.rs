use std::time::Duration;

use ptytest::{ExitStatus, PtyTest, PtyTestError};

const PROCFS_EXIT_RACE_ATTEMPTS: usize = 5;

// laputa-ptytest 0.1.1 to 0.3.0 scan /proc for process-group members while
// reaping and treat ESRCH from reading /proc/<pid>/stat as fatal. ESRCH means
// an unrelated process exited between listing /proc and reading its stat, so
// the scan is retried; the reap itself is idempotent until it succeeds.
fn is_procfs_exit_race(error: &PtyTestError) -> bool {
    matches!(
        error,
        PtyTestError::Io { operation: "read procfs stat", source, .. }
            if source.raw_os_error() == Some(libc::ESRCH)
    )
}

pub fn reap(terminal: &mut PtyTest, timeout: Duration) -> Result<ExitStatus, PtyTestError> {
    let mut attempt = 1;
    loop {
        match terminal.finish(terminal.deadline(timeout)) {
            Err(error) if is_procfs_exit_race(&error) && attempt < PROCFS_EXIT_RACE_ATTEMPTS => {
                attempt += 1;
            }
            result => return result,
        }
    }
}
