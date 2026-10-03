use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Starting,
    Running,
    Idle,
    Stopping,
    Stopped,
    Warning,
    Building,
    Error,
    Unknown,
}

#[derive(Debug)]
pub struct State {
    pub stderr: Vec<(u64, String)>,
    pub data: String,
    pub status: Status,
}

pub fn update_state(_state: &mut State) -> std::result::Result<(), String> {
    Ok(())
}

pub fn format_diag(msg: &str) -> String {
    format!("[artisan] {}", msg)
}

pub fn diag(state: &mut State, msg: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    state.stderr.push((ts, format_diag(msg)));
    state.data = msg.to_string();
    state.status = Status::Error;
    let _ = update_state(state);
}

pub const EXIT_RESTART: i32 = 75;

#[derive(Debug, PartialEq)]
pub enum Backoff {
    Wait(Duration),
    GiveUp,
}

pub fn next_delay(history: &[std::time::Instant], now: std::time::Instant) -> Backoff {
    let window = Duration::from_secs(120);
    let recent: Vec<_> = history
        .iter()
        .filter(|&&t| {
            let d = now.duration_since(t);
            d <= window
        })
        .collect();

    let mut count = recent.len();

    if count > 0 {
        let oldest_recent = recent.first().unwrap();
        let oldest_recent_time = *oldest_recent;

        // Find the most recent crash older than oldest_recent
        let prev_older = history.iter().rev().find(|&&t| t < *oldest_recent_time);

        if let Some(&prev_older_time) = prev_older {
            let gap = oldest_recent_time.duration_since(prev_older_time);
            if gap > window {
                // Sequence reset: the oldest recent crash is the start of a new sequence
                count = count.saturating_sub(1);
            }
        }
    }

    if count >= 5 {
        return Backoff::GiveUp;
    }

    let delay_secs = match count {
        0 => 5,
        1 => 10,
        2 => 20,
        3 => 40,
        _ => 60,
    };

    Backoff::Wait(Duration::from_secs(delay_secs))
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
    match timeout(Duration::from_secs(1), TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => true,
        _ => false,
    }
}
