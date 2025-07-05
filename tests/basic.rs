use ais_runner::child::{create_child};
use ais_runner::config::{generate_application_state};
use artisan_middleware::config::AppConfig;
use artisan_middleware::state_persistence::{StatePersistence, update_state};
use ais_runner::config::AppSpecificConfig;
use tempfile::tempdir;
use tokio::time::{sleep, Duration};

#[tokio::test]
async fn spawn_and_kill_child() {
    let dir = tempdir().unwrap();
    let settings = AppSpecificConfig {
        interval_seconds: 1,
        monitor_path: dir.path().to_str().unwrap().to_string(),
        project_path: dir.path().to_str().unwrap().to_string(),
        changes_needed: 1,
        ignored_subdirs: vec![],
        install_command: None,
        build_command: None,
        run_command: "sleep 5".to_string(),
    };
    let config = AppConfig::dummy();
    let state_path = StatePersistence::get_state_path(&config);
    let mut state = generate_application_state(&state_path, &config).await;

    let mut child = create_child(&mut state, &state_path, &settings).await;
    assert!(child.running().await);

    child.kill().await.unwrap();
    sleep(Duration::from_millis(100)).await;
    assert!(!child.running().await);
}

#[tokio::test]
async fn collect_log_data() {
    let dir = tempdir().unwrap();
    let settings = AppSpecificConfig {
        interval_seconds: 1,
        monitor_path: dir.path().to_str().unwrap().to_string(),
        project_path: dir.path().to_str().unwrap().to_string(),
        changes_needed: 1,
        ignored_subdirs: vec![],
        install_command: None,
        build_command: None,
        run_command: "sh -c 'echo hello'".to_string(),
    };
    let config = AppConfig::dummy();
    let state_path = StatePersistence::get_state_path(&config);
    let mut state = generate_application_state(&state_path, &config).await;

    let mut child = create_child(&mut state, &state_path, &settings).await;
    sleep(Duration::from_millis(200)).await;
    let out = child.get_std_out().await.unwrap();
    child.kill().await.ok();
    let found = out.iter().any(|(_, line)| line.contains("hello"));
    assert!(found);
}

#[tokio::test]
async fn update_state_increments_counter() {
    let config = AppConfig::dummy();
    let state_path = StatePersistence::get_state_path(&config);
    let mut state = generate_application_state(&state_path, &config).await;

    let prev_counter = state.event_counter;
    let prev_timestamp = state.last_updated;

    update_state(&mut state, &state_path, None).await;

    assert_eq!(state.event_counter, prev_counter + 1);
    assert!(state.last_updated >= prev_timestamp);
}
