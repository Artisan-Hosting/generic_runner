use std::time::{Duration, Instant};

use ais_runner::diagnostics::{diag, format_diag, next_delay, port_check_message, Backoff, State, EXIT_RESTART};

#[test]
fn diagnostics_format_diag_prefixes_message() {
    assert_eq!(format_diag("build failed"), "[artisan] build failed");
}

#[test]
fn diagnostics_format_diag_empty_message_yields_bare_prefix() {
    assert_eq!(format_diag(""), "[artisan] ");
}

#[test]
fn diagnostics_diag_pushes_prefixed_line_with_timestamp() {
    let mut state = State {
        stderr: Vec::new(),
        data: String::new(),
        status: "Starting".to_string(),
    };
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    diag(&mut state, "install failed");
    let after = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(state.stderr.len(), 1);
    let (ts, line) = &state.stderr[0];
    assert_eq!(line, "[artisan] install failed");
    assert!(*ts >= before && *ts <= after, "timestamp {} not current", ts);
}

#[test]
fn diagnostics_diag_sets_data_to_raw_message() {
    let mut state = State {
        stderr: Vec::new(),
        data: String::new(),
        status: "Starting".to_string(),
    };
    diag(&mut state, "install failed");
    assert_eq!(state.data, "install failed");
}

#[test]
fn diagnostics_diag_calls_update_state_transitioning_status() {
    let mut state = State {
        stderr: Vec::new(),
        data: String::new(),
        status: "Starting".to_string(),
    };
    diag(&mut state, "install failed");
    assert_eq!(state.status, "Error");
}

#[test]
fn diagnostics_exit_restart_constant_is_75() {
    assert_eq!(EXIT_RESTART, 75);
}

#[test]
fn diagnostics_first_crash_waits_5s() {
    let now = Instant::now();
    assert_eq!(next_delay(&[], now), Backoff::Wait(Duration::from_secs(5)));
}

#[test]
fn diagnostics_second_crash_waits_10s() {
    let t0 = Instant::now();
    let now = t0 + Duration::from_secs(3);
    assert_eq!(next_delay(&[t0], now), Backoff::Wait(Duration::from_secs(10)));
}

#[test]
fn diagnostics_third_crash_waits_20s() {
    let t0 = Instant::now();
    let t1 = t0 + Duration::from_secs(3);
    let now = t1 + Duration::from_secs(3);
    assert_eq!(next_delay(&[t0, t1], now), Backoff::Wait(Duration::from_secs(20)));
}

#[test]
fn diagnostics_fourth_crash_waits_40s() {
    let t0 = Instant::now();
    let t1 = t0 + Duration::from_secs(3);
    let t2 = t1 + Duration::from_secs(3);
    let now = t2 + Duration::from_secs(3);
    assert_eq!(next_delay(&[t0, t1, t2], now), Backoff::Wait(Duration::from_secs(40)));
}
