//! Writes a shell script that a test is about to execute, and returns only
//! once the kernel will actually exec it.
//!
//! On Linux a sibling test thread that forks while the script is still open
//! for writing inherits the write descriptor until its own exec, and an exec
//! of the script in that window fails with ETXTBSY ("Text file busy"). The
//! window is closed by probing: the script starts with a guard line that
//! makes it exit at once when `OUTOU_TEST_EXEC_PROBE` is set, and the probe
//! is retried while the exec fails with `ExecutableFileBusy`. A probe that
//! runs means no forked child holds the file open any more, and none can
//! inherit the descriptor afterwards because this one is already closed.

use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::Duration;

const PROBE_ENV: &str = "OUTOU_TEST_EXEC_PROBE";
const MAX_PROBES: usize = 50;
const PROBE_BACKOFF: Duration = Duration::from_millis(20);

/// Writes `#!/bin/sh`, the probe guard and `body` to `path`, makes it
/// executable and waits until it can be exec'd. `body` has no shebang.
pub fn write_script(path: &Path, body: &str) {
    let text = format!("#!/bin/sh\n[ -n \"${PROBE_ENV}\" ] && exit 0\n{body}");
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    wait_until_executable(path);
}

fn wait_until_executable(path: &Path) {
    for _ in 0..MAX_PROBES {
        let probe = Command::new(path)
            .env(PROBE_ENV, "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if !is_text_file_busy(&probe) {
            probe.unwrap_or_else(|e| panic!("probing {}: {e}", path.display()));
            return;
        }
        sleep(PROBE_BACKOFF);
    }
    panic!(
        "{} stayed busy (ETXTBSY) for {MAX_PROBES} probes",
        path.display()
    );
}

/// Whether `result` is the ETXTBSY failure to exec a file held open for writing.
pub fn is_text_file_busy<T>(result: &std::io::Result<T>) -> bool {
    matches!(result, Err(e) if e.kind() == ErrorKind::ExecutableFileBusy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_busy_error_kind_is_retried() {
        let busy: std::io::Result<()> = Err(ErrorKind::ExecutableFileBusy.into());
        let other: std::io::Result<()> = Err(ErrorKind::NotFound.into());
        assert!(is_text_file_busy(&busy));
        assert!(!is_text_file_busy(&other));
        assert!(!is_text_file_busy(&Ok(())));
    }
}
