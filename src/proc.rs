//! Run an external program with a timeout and a cap on captured output.
//!
//! Callers build the `Command` (program, arguments, environment). This
//! module adds the guard rails every child process gets:
//!
//! - no stdin;
//! - both output pipes drained on their own threads, so a chatty child
//!   cannot stall;
//! - a hard limit on how much stdout is kept;
//! - a wall-clock timeout for the whole operation, including waiting for
//!   the output pipes to close;
//! - the child runs in its own process group, and on timeout or overflow
//!   the whole group is killed. Killing only the child would leave
//!   grandchildren holding the pipes open.

use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Most stderr that is kept. Anything beyond it is read and discarded.
const MAX_STDERR: usize = 64 * 1024;

/// Minimum time given to the output readers to finish after the child exits.
const READER_GRACE: Duration = Duration::from_millis(250);

#[derive(Debug)]
pub struct Captured {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug)]
pub enum ProcError {
    /// The program could not be started (for example, not installed).
    Spawn(io::Error),
    /// Waiting on the process failed.
    Io(io::Error),
    /// The program did not finish in time and was killed.
    Timeout(Duration),
    /// Stdout exceeded the limit; the program was killed.
    OutputTooLarge { limit: usize },
}

/// Read up to `max` bytes. When more arrives, set `overflow` and stop.
fn read_capped<R: Read>(mut r: R, max: usize, overflow: &AtomicBool) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match r.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() + n > max {
                    let room = max - buf.len();
                    buf.extend_from_slice(&chunk[..room]);
                    overflow.store(true, Ordering::SeqCst);
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    buf
}

/// Keep the first `max` bytes and silently discard the rest, reading to EOF
/// so the child never blocks on a full pipe.
fn read_truncated<R: Read>(mut r: R, max: usize) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match r.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buf.len() < max {
                    let room = (max - buf.len()).min(n);
                    buf.extend_from_slice(&chunk[..room]);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
    buf
}

/// Kill the child and everything in its process group, then reap it.
fn kill_tree(child: &mut Child) {
    // The child was started with `process_group(0)`, so its pid is the id of
    // its group, and a negative pid addresses the whole group.
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: `kill` has no memory-safety preconditions; at worst the
        // group is already gone and the call fails harmlessly.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn collect(rx: &Receiver<Vec<u8>>, deadline: Instant) -> Option<Vec<u8>> {
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .max(READER_GRACE);
    rx.recv_timeout(remaining).ok()
}

pub fn run_capture(
    mut cmd: Command,
    timeout: Duration,
    max_stdout: usize,
) -> Result<Captured, ProcError> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .map_err(ProcError::Spawn)?;

    let out_pipe = child.stdout.take().expect("stdout is piped");
    let err_pipe = child.stderr.take().expect("stderr is piped");
    let overflow = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&overflow);
    let (out_tx, out_rx) = mpsc::channel();
    let (err_tx, err_rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = out_tx.send(read_capped(out_pipe, max_stdout, &flag));
    });
    thread::spawn(move || {
        let _ = err_tx.send(read_truncated(err_pipe, MAX_STDERR));
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        if overflow.load(Ordering::SeqCst) {
            kill_tree(&mut child);
            return Err(ProcError::OutputTooLarge { limit: max_stdout });
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_tree(&mut child);
                    return Err(ProcError::Timeout(timeout));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(e) => {
                kill_tree(&mut child);
                return Err(ProcError::Io(e));
            }
        }
    };

    // The child has exited, but a descendant may still hold the pipes open.
    // Wait for the readers only until the deadline, then clean up.
    let stdout = collect(&out_rx, deadline);
    let stderr = collect(&err_rx, deadline);
    let (stdout, stderr) = match (stdout, stderr) {
        (Some(o), Some(e)) => (o, e),
        _ => {
            kill_tree(&mut child);
            return Err(ProcError::Timeout(timeout));
        }
    };

    if overflow.load(Ordering::SeqCst) {
        return Err(ProcError::OutputTooLarge { limit: max_stdout });
    }
    Ok(Captured {
        status,
        stdout,
        stderr,
    })
}
