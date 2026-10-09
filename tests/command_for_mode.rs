#[cfg(test)]
mod command_for_mode_tests {
    use super::*;
    use ais_runner::child::{command_for_mode, CommandKind};
    use tokio::process::Command;

    fn assert_cmd_eq(cmd: &Command, expected_program: &str, expected_args: &[&str]) {
        let std = cmd.as_std();
        assert_eq!(std.get_program(), expected_program);
        assert_eq!(
            std.get_args().map(|a| a.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            expected_args
        );
    }

    // case cmd_003 begin
    #[test]
    fn case_cmd_003() {
        let cmd = command_for_mode(CommandKind::Run, "npm start", "/app", &[("VAR".to_owned(), "VAL".to_owned())], true).unwrap();
        assert_cmd_eq(&cmd, "/bin/sh", &["-c", "exec npm start"]);
    }
    // case cmd_003 end

    // case cmd_004 begin
    #[test]
    fn case_cmd_004() {
        let cmd = command_for_mode(CommandKind::Install, "npm ci && npm run build", "/app", &[("VAR".to_owned(), "VAL".to_owned())], false).unwrap();
        assert_cmd_eq(&cmd, "npm", &["ci", "&&", "npm", "run", "build"]);
    }
    // case cmd_004 end



    // case cmd_017 begin
    #[test]
    fn case_cmd_017() {
        let cmd = command_for_mode(CommandKind::Run, "python app.py", "/app", &[("VAR".to_owned(), "VAL".to_owned())], true).unwrap();
        assert_cmd_eq(&cmd, "/bin/sh", &["-c", "exec python app.py"]);
    }
    // case cmd_017 end

    // case cmd_018 begin
    #[test]
    fn case_cmd_018() {
        let cmd = command_for_mode(CommandKind::Run, "python app.py", "/app", &[("VAR".to_owned(), "VAL".to_owned())], false).unwrap();
        assert_cmd_eq(&cmd, "python", &["app.py"]);
    }
    // case cmd_018 end

    // case cmd_021 begin
    #[test]
    fn case_cmd_021() {
        let cmd = command_for_mode(CommandKind::Run, "gunicorn --bind 0.0.0.0:$PORT app:app", "/app", &[("PORT".to_owned(), "8000".to_owned())], true).unwrap();
        assert_cmd_eq(&cmd, "/bin/sh", &["-c", "exec gunicorn --bind 0.0.0.0:$PORT app:app"]);
    }
    // case cmd_021 end

    // case g1_run_002a_a1_1 begin
    #[test]
    fn case_g1_run_002a_a1_1() {
        let cmd = command_for_mode(CommandKind::Install, "npm ci && npm run build", "/app", &[], true).unwrap();
        assert_cmd_eq(&cmd, "/bin/sh", &["-c", "npm ci && npm run build"]);
    }
    // case g1_run_002a_a1_1 end

    // case g1_run_002a_a1_2 begin
    #[test]
    fn case_g1_run_002a_a1_2() {
        let cmd = command_for_mode(CommandKind::Build, "npm ci && npm run build", "/app", &[], true).unwrap();
        assert_cmd_eq(&cmd, "/bin/sh", &["-c", "npm ci && npm run build"]);
    }
    // case g1_run_002a_a1_2 end

    // case g1_run_002a_a1_3 begin
    #[test]
    fn case_g1_run_002a_a1_3() {
        let cmd = command_for_mode(CommandKind::Install, "make install", "/app", &[], true).unwrap();
        assert_cmd_eq(&cmd, "/bin/sh", &["-c", "make install"]);
    }
    // case g1_run_002a_a1_3 end
}
