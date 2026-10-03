mod diagnostics {
    use std::time::{Duration, Instant};

    use ais_runner::diagnostics::{
        diag, format_diag, next_delay, port_check_message, Backoff, State, Status,
    };

    fn fresh_state() -> State {
        State {
            stderr: Vec::new(),
            data: String::new(),
            status: Status::Starting,
        }
    }

    #[test]
    fn tc_run3_01_format_diag_prefixes_message() {
        assert_eq!(format_diag("build failed"), "[artisan] build failed");
    }

    #[test]
    fn tc_run3_02_format_diag_empty_message_bare_prefix() {
        assert_eq!(format_diag(""), "[artisan] ");
    }

    #[test]
    fn tc_run3_03_diag_pushes_prefixed_line_with_timestamp() {
        let mut state = fresh_state();
        diag(&mut state, "install failed");
        assert_eq!(state.stderr.len(), 1);
        let (ts, line) = &state.stderr[0];
        assert_eq!(line, "[artisan] install failed");
        assert!(*ts > 0, "timestamp should be the current time");
    }

    #[test]
    fn tc_run3_04_diag_sets_data_to_raw_message() {
        let mut state = fresh_state();
        state.data = "old".to_string();
        diag(&mut state, "install failed");
        assert_eq!(state.data, "install failed");
    }

    #[test]
    fn tc_run3_05_diag_calls_update_state_leading_to_error_status() {
        let mut state = fresh_state();
        diag(&mut state, "build failed");
        assert_eq!(state.status, Status::Error);
    }

    #[test]
    fn tc_run3_07_first_crash_waits_five_seconds() {
        let now = Instant::now();
        assert_eq!(next_delay(&[], now), Backoff::Wait(Duration::from_secs(5)));
    }

    #[test]
    fn tc_run3_08_successive_crashes_escalate() {
        let now = Instant::now();
        let h1 = vec![now - Duration::from_secs(1)];
        assert_eq!(
            next_delay(&h1, now),
            Backoff::Wait(Duration::from_secs(10))
        );
        let h2 = vec![
            now - Duration::from_secs(2),
            now - Duration::from_secs(1),
        ];
        assert_eq!(
            next_delay(&h2, now),
            Backoff::Wait(Duration::from_secs(20))
        );
        let h3 = vec![
            now - Duration::from_secs(3),
            now - Duration::from_secs(2),
            now - Duration::from_secs(1),
        ];
        assert_eq!(
            next_delay(&h3, now),
            Backoff::Wait(Duration::from_secs(40))
        );
        let h4 = vec![
            now - Duration::from_secs(4),
            now - Duration::from_secs(3),
            now - Duration::from_secs(2),
            now - Duration::from_secs(1),
        ];
        assert_eq!(
            next_delay(&h4, now),
            Backoff::Wait(Duration::from_secs(60))
        );
    }

    #[test]
    fn tc_run3_09_five_crashes_within_120s_give_up() {
        let now = Instant::now();
        let history = vec![
            now - Duration::from_secs(50),
            now - Duration::from_secs(40),
            now - Duration::from_secs(30),
            now - Duration::from_secs(20),
            now - Duration::from_secs(10),
        ];
        assert_eq!(next_delay(&history, now), Backoff::GiveUp);
    }

    #[test]
    fn tc_run3_10_crashes_older_than_120s_do_not_count() {
        let now = Instant::now();
        let history = vec![
            now - Duration::from_secs(200),
            now - Duration::from_secs(190),
            now - Duration::from_secs(180),
            now - Duration::from_secs(170),
            now - Duration::from_secs(160),
        ];
        assert_eq!(
            next_delay(&history, now),
            Backoff::Wait(Duration::from_secs(5))
        );
    }

    #[test]
    fn tc_run3_11_crash_after_long_uptime_resets_history() {
        let now = Instant::now();
        let history = vec![
            now - Duration::from_secs(300),
            now - Duration::from_secs(250),
            now - Duration::from_secs(200),
            now - Duration::from_secs(150),
            now - Duration::from_secs(1),
        ];
        assert_eq!(
            next_delay(&history, now),
            Backoff::Wait(Duration::from_secs(5))
        );
    }

    #[test]
    fn tc_run3_12_port_check_message_not_listening() {
        assert_eq!(
            port_check_message(3000, false),
            Some(
                "nothing is listening on PORT=3000 after 60s; your app must listen on 0.0.0.0:$PORT"
                    .to_string()
            )
        );
    }

    #[test]
    fn tc_run3_13_port_check_message_listening_is_none() {
        assert_eq!(port_check_message(3000, true), None);
    }
}
