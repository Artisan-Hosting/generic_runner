use std::time::{Duration, Instant};

#[derive(Debug, PartialEq)]
pub enum Backoff {
    Wait(Duration),
    GiveUp,
}

pub const EXIT_RESTART: i32 = 75;

/// Minimal state shape used by the diagnostics module.
pub struct State {
    pub stderr: Vec<(u64, String)>,
    pub data: String,
    pub status: String,
}

pub fn format_diag(msg: &str) -> String {
    format!("[artisan] {}", msg)
}

pub fn diag(state: &mut State, msg: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    state.stderr.push((ts, format_diag(msg)));
    state.data = msg.to_string();
    state.status = "Error".to_string();
    update_state(state);
}

pub fn update_state(_state: &mut State) {
    // Placeholder: main.rs handles actual persistence
}

pub fn next_delay(history: &[Instant], now: Instant) -> Backoff {
    const MAX_HISTORY_AGE: Duration = Duration::from_secs(120);
    const MAX_CRASHES: usize = 5;
    const DELAYS: [Duration; 5] = [
        Duration::from_secs(5),
        Duration::from_secs(10),
        Duration::from_secs(20),
        Duration::from_secs(40),
        Duration::from_secs(60),
    ];

    let mut recent_crashes = 0;
    for &t in history {
        if now.duration_since(t) <= MAX_HISTORY_AGE {
            recent_crashes += 1;
        }
    }

    if recent_crashes >= MAX_CRASHES {
        return Backoff::GiveUp;
    }

    let delay = DELAYS[recent_crashes];
    Backoff::Wait(delay)
}

pub fn port_check_message(port: u16, listening: bool) -> Option<String> {
    if listening {
        None
    } else {
        Some(format!(
            "nothing is listening on PORT={} after 60s; your app must listen on 0.0.0.0:$PORT",
            port
        ))
    }
}

pub async fn is_listening(port: u16) -> bool {
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    let addr = format!("127.0.0.1:{}", port);
    let result = timeout(Duration::from_millis(100), TcpStream::connect(&addr)).await;
    result.is_ok() && result.unwrap().is_ok()
}
