//! A native build made with `bif-tui` has the terminal UI classes: a compiled program can create the TUI object and draw on
//! its canvas (`createObject("rust", "tui.TUI")`), like `matchbox script.bxs` does.
#![cfg(feature = "bif-tui")]

use std::{fs, process::Command};

#[test]
fn a_native_program_can_draw_on_the_tui_canvas() {
    let dir = std::env::temp_dir().join(format!("matchbox-native-tui-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("app.bxs"),
        r#"tui = createObject( "rust", "tui.TUI" );
tui.begin_frame();
tui.border( 0, 0, 12, 3, { border: "rounded", fg: "#51576d", title: "Hi" } );
tui.gradient( 2, 1, "native", "#00dbff", "#00ff78", { bold: true } );
println( tui.render_to_string( 12, 3 ) );
"#,
    )
    .unwrap();
    let binary = dir.join("app");
    let build = Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .current_dir(&dir)
        .args(["--target", "native", "--output", binary.to_str().unwrap(), "app.bxs"])
        .output()
        .unwrap();
    assert!(build.status.success(), "{}", String::from_utf8_lossy(&build.stderr));

    let run = Command::new(&binary).current_dir(&dir).output().unwrap();
    let out = String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success(), "{}{}", out, String::from_utf8_lossy(&run.stderr));
    assert!(out.contains("╭─ Hi ─────╮"), "{out}");
    assert!(out.contains("│ native   │"), "{out}");
    assert!(out.contains("╰──────────╯"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}
