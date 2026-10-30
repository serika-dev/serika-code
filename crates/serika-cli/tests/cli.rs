use std::process::Command;

#[test]
fn version_help_and_session_list() {
    let exe = env!("CARGO_BIN_EXE_serika");
    let version = Command::new(exe).arg("--version").output().unwrap();
    assert!(version.status.success());
    let text = String::from_utf8_lossy(&version.stdout);
    assert!(text.contains("serika"));

    let help = Command::new(exe).arg("--help").output().unwrap();
    assert!(help.status.success());
    let help_text = String::from_utf8_lossy(&help.stdout);
    assert!(help_text.contains("doctor"));
    assert!(help_text.contains("auth"));

    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let listed = Command::new(exe)
        .args(["sessions", "list"])
        .env("SERIKA_HOME", home.path())
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(
        listed.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let listed_text = String::from_utf8_lossy(&listed.stdout);
    assert!(listed_text.contains("no sessions"));
}

#[test]
fn config_roundtrip() {
    let exe = env!("CARGO_BIN_EXE_serika");
    let home = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let set = Command::new(exe)
        .args(["config", "set", "permission", "restricted"])
        .env("SERIKA_HOME", home.path())
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(set.status.success(), "{}", String::from_utf8_lossy(&set.stderr));
    let show = Command::new(exe)
        .args(["config", "show"])
        .env("SERIKA_HOME", home.path())
        .current_dir(project.path())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&show.stdout);
    assert!(text.contains("restricted"), "{text}");
}
