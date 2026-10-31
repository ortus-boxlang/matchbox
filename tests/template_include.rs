use std::process::Command;

#[test]
fn include_tag_renders_the_named_template() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("part.bxm"), "part").unwrap();
    std::fs::write(
        dir.path().join("page.bxm"),
        r#"before <bx:include template="part.bxm"> after"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .arg(dir.path().join("page.bxm"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "before part after");
}

#[test]
fn include_tag_requires_a_template_attribute() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("page.bxm"), "<bx:include>").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .arg(dir.path().join("page.bxm"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires a template attribute"));
}
