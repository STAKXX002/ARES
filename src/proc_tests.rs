use crate::proc::{run_capture, ProcError};
use std::process::Command;
use std::time::{Duration, Instant};

fn sh(script: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(script);
    c
}

const T: Duration = Duration::from_secs(10);

#[test]
fn captures_stdout_stderr_and_exit_status() {
    let out = run_capture(sh("echo out; echo err >&2; exit 3"), T, 1024).unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "out\n");
    assert_eq!(String::from_utf8_lossy(&out.stderr), "err\n");
}

#[test]
fn a_program_that_hangs_is_killed_at_the_timeout() {
    let start = Instant::now();
    match run_capture(sh("sleep 30"), Duration::from_millis(200), 1024) {
        Err(ProcError::Timeout(d)) => assert_eq!(d, Duration::from_millis(200)),
        other => panic!("expected a timeout, got {other:?}"),
    }
    assert!(start.elapsed() < Duration::from_secs(5), "the child must be killed promptly");
}

#[test]
fn output_over_the_limit_is_an_error_and_the_child_is_killed() {
    let start = Instant::now();
    match run_capture(sh("head -c 5000000 /dev/zero"), T, 10_000) {
        Err(ProcError::OutputTooLarge { limit }) => assert_eq!(limit, 10_000),
        other => panic!("expected OutputTooLarge, got {other:?}"),
    }
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_child_that_never_stops_writing_is_still_stopped() {
    let start = Instant::now();
    let r = run_capture(sh("yes"), T, 100_000);
    assert!(matches!(r, Err(ProcError::OutputTooLarge { .. })), "{r:?}");
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn large_output_under_the_limit_arrives_complete_without_deadlock() {
    let out = run_capture(sh("head -c 500000 /dev/zero"), T, 1_000_000).unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), 500_000);
}

#[test]
fn output_exactly_at_the_limit_is_accepted() {
    let out = run_capture(sh("head -c 1000 /dev/zero"), T, 1000).unwrap();
    assert_eq!(out.stdout.len(), 1000);
}

#[test]
fn a_flood_on_stderr_neither_blocks_nor_fails_the_run() {
    let out = run_capture(sh("head -c 2000000 /dev/zero >&2; echo done"), T, 1024).unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "done\n");
    assert!(out.stderr.len() <= 64 * 1024, "stderr must be truncated");
    assert!(!out.stderr.is_empty());
}

#[test]
fn a_missing_program_is_a_spawn_error() {
    let r = run_capture(Command::new("/nonexistent/definitely-not-here"), T, 1024);
    assert!(matches!(r, Err(ProcError::Spawn(_))), "{r:?}");
}

#[test]
fn stdin_is_closed_so_programs_that_read_it_do_not_hang() {
    let start = Instant::now();
    let out = run_capture(sh("cat"), T, 1024).unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(start.elapsed() < Duration::from_secs(5));
}

#[test]
fn a_nonzero_exit_is_reported_not_treated_as_an_error_here() {
    let out = run_capture(sh("exit 1"), T, 1024).unwrap();
    assert!(!out.status.success());
}

#[test]
fn grandchildren_are_killed_with_the_child_on_timeout() {
    // `sleep` runs in the background and keeps the pipes open after `sh`
    // would be killed. The whole group must go.
    let start = Instant::now();
    let r = run_capture(sh("sleep 30 & sleep 30"), Duration::from_millis(200), 1024);
    assert!(matches!(r, Err(ProcError::Timeout(_))), "{r:?}");
    assert!(start.elapsed() < Duration::from_secs(5), "took {:?}", start.elapsed());
}

#[test]
fn a_leftover_background_process_cannot_hold_the_call_open() {
    // `sh` exits at once but a background `sleep` still owns stdout.
    let start = Instant::now();
    let r = run_capture(sh("sleep 30 & echo hi"), Duration::from_millis(300), 1024);
    assert!(matches!(r, Err(ProcError::Timeout(_))), "{r:?}");
    assert!(start.elapsed() < Duration::from_secs(5), "took {:?}", start.elapsed());
}

#[test]
fn a_child_in_its_own_group_does_not_receive_our_terminal_signals() {
    // Smoke test that spawning with a separate process group still works
    // normally for ordinary programs.
    let out = run_capture(sh("echo grouped"), T, 1024).unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "grouped\n");
}
