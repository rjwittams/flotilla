use std::process::Command;

#[test]
fn installed_package_exposes_flotillad_binary() {
    let flotillad = std::env::var("CARGO_BIN_EXE_flotillad").expect("root package should build a flotillad binary target");
    let status = Command::new(flotillad).arg("--help").status().expect("flotillad help should run");

    assert!(status.success(), "flotillad --help should succeed");
}
