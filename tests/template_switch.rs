use std::process::Command;

fn render(template: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("page.bxm");
    std::fs::write(&path, template).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_matchbox")).arg(&path).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn switch_on(value: &str) -> String {
    render(&format!(
        r##"<bx:set x = {value}>
<bx:switch expression="#x#">
    <bx:case value="3">three</bx:case>
    <bx:case value="a,b" delimiters=",">letter</bx:case>
    <bx:defaultcase>other</bx:defaultcase>
</bx:switch>"##
    ))
}

#[test]
fn matching_case_runs_and_default_is_skipped() {
    assert_eq!(switch_on("3"), "three");
}

#[test]
fn delimited_case_values_match_any_item() {
    assert_eq!(switch_on(r#""a""#), "letter");
    assert_eq!(switch_on(r#""b""#), "letter");
}

#[test]
fn default_case_runs_when_nothing_matches() {
    assert_eq!(switch_on("99"), "other");
}

#[test]
fn text_after_the_switch_is_kept() {
    assert_eq!(
        render(r##"<bx:set x = 3><bx:switch expression="#x#"><bx:case value="3">three</bx:case><bx:defaultcase>other</bx:defaultcase></bx:switch> after"##),
        "three after"
    );
}
