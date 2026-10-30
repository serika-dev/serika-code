use std::process::Command;

#[test]
fn prints_version_without_opening_a_window() {
    let exe = env!("CARGO_BIN_EXE_serika-desktop");
    let output = Command::new(exe).arg("--version").output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("serika-desktop"));
}
