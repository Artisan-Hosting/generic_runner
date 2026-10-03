use std::time::Duration;

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

pub fn update_state(_state: &mut State) {
    todo!()
}

pub fn format_diag(_msg: &str) -> String {
    todo!()
}

pub fn diag(_state: &mut State, _msg: &str) {
    todo!()
}

pub const EXIT_RESTART: i32 = 75;

#[derive(Debug, PartialEq)]
pub enum Backoff {
    Wait(Duration),
    GiveUp,
}

pub fn next_delay(_history: &[std::time::Instant], _now: std::time::Instant) -> Backoff {
    todo!()
}

pub fn port_check_message(_port: u16, _listening: bool) -> Option<String> {
    todo!()
}

pub async fn is_listening(_port: u16) -> bool {
    todo!()
}
