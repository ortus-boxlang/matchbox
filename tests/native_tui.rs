//! A native build made with `bif-tui` has the terminal UI classes: a compiled program can create the app, the
//! terminal and the widgets (`createObject("rust", "tui.List")` and friends), like `matchbox script.bxs` does.
#![cfg(feature = "bif-tui")]

use std::{fs, process::Command};

#[test]
fn a_native_program_can_build_terminal_ui_widgets() {
    let dir = std::env::temp_dir().join(format!("matchbox-native-tui-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("app.bxs"),
        r#"app = createObject( "rust", "tui.App" );
list = createObject( "rust", "tui.List" ).addItem( "one" ).addItem( "two" ).selected( 1 );
input = createObject( "rust", "tui.Input" ).prompt( "name: " ).value( "luis" );
box = createObject( "rust", "tui.VBox" ).add( list ).add( input );
println( "built=" & ( box.build() > 0 ) );
app.stop();
println( "quit=" & app.isQuit() );
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
    assert!(out.contains("built=true"), "{out}");
    assert!(out.contains("quit=true"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}
