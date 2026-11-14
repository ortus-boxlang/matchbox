//! `tui.image` and `tui.image_support` on the canvas, run through the script interpreter: a picture is painted as half blocks in
//! `render_to_string`, a missing file returns false and draws nothing, and the protocol comes from the environment.
#![cfg(feature = "bif-tui")]

use std::{fs, process::Command};

/// A 2 x 4 pixel PNG: red, green / blue, white / black, yellow / cyan, magenta.
const FOUR_ROWS: &[u8] = &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x04, 0x08, 0x02, 0x00, 0x00, 0x00, 0x2b, 0x8d, 0x79, 0x6e, 0x00, 0x00, 0x00, 0x17, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0xc0, 0x00, 0xc2, 0x0c, 0xff, 0x81, 0x00, 0x42, 0x43, 0x38, 0x0c, 0xff, 0x01, 0x93, 0x88, 0x0b, 0xf5, 0x0c, 0xe8, 0x91, 0x3d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82];

fn run(dir: &std::path::Path, script: &str, env: &[(&str, &str)], remove: &[&str]) -> String {
    fs::write(dir.join("t.bxs"), script).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_matchbox"));
    cmd.current_dir(dir).arg("t.bxs");
    for key in ["TERM", "TERM_PROGRAM", "KITTY_WINDOW_ID", "GHOSTTY_RESOURCES_DIR", "TMUX", "STY", "MATCHBOX_TUI_IMAGES"] {
        cmd.env_remove(key);
    }
    for key in remove {
        cmd.env_remove(key);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "{text}{}", String::from_utf8_lossy(&out.stderr));
    text
}

fn workdir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("matchbox-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("four-rows.png"), FOUR_ROWS).unwrap();
    dir
}

#[test]
fn a_picture_shows_in_render_to_string_as_half_blocks_and_text_lands_on_top() {
    let dir = workdir("tui-image-draw");
    let out = run(
        &dir,
        r##"tui = createObject( "rust", "tui.TUI" );
tui.begin_frame();
ok = tui.image( 1, 0, 2, 2, "four-rows.png" );
tui.text( 2, 1, "Z", {} );
println( "ok=" & ok );
println( tui.render_to_string( 5, 3 ) );
"##,
        &[],
        &[],
    );
    assert!(out.contains("ok=true"), "{out}");
    assert!(out.contains(" \u{2580}\u{2580}  "), "{out}");
    assert!(out.contains(" \u{2580}Z  "), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_file_returns_false_and_draws_nothing_and_options_are_checked() {
    let dir = workdir("tui-image-missing");
    let out = run(
        &dir,
        r##"tui = createObject( "rust", "tui.TUI" );
tui.begin_frame();
println( "missing=" & tui.image( 0, 0, 3, 2, "nope.png" ) );
println( "cover=" & tui.image( 0, 0, 3, 2, "four-rows.png", { fit: "cover", protocol: "kitty" } ) );
println( "sixel=" & tui.image( 0, 0, 3, 2, "four-rows.png", { protocol: "sixel" } ) );
try { tui.image( 0, 0, 3, 2, "four-rows.png", { protocol: "nope" } ); println( "no error" ); } catch ( any e ) { println( "error: " & e.message ); }
try { tui.image( 0, 0, 3 ); println( "no error" ); } catch ( any e ) { println( "arity: " & e.message ); }
println( tui.render_to_string( 3, 2 ) );
"##,
        &[],
        &[],
    );
    assert!(out.contains("missing=false"), "{out}");
    assert!(out.contains("cover=true"), "{out}");
    assert!(out.contains("sixel=true"), "{out}");
    assert!(out.contains("error: Unknown image protocol: nope"), "{out}");
    assert!(out.contains("arity: image requires 5 or 6 arguments"), "{out}");
    assert!(out.contains("\u{2580}\u{2580}\u{2580}"), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn image_support_follows_the_environment() {
    let dir = workdir("tui-image-support");
    let script = r##"tui = createObject( "rust", "tui.TUI" );
println( "support=" & tui.image_support() );
"##;
    assert!(run(&dir, script, &[("TERM", "xterm-256color")], &[]).contains("support=halfblocks"));
    assert!(run(&dir, script, &[("TERM", "xterm-kitty")], &[]).contains("support=kitty"));
    assert!(run(&dir, script, &[("TERM", "xterm-ghostty")], &[]).contains("support=kitty"));
    assert!(run(&dir, script, &[("TERM_PROGRAM", "ghostty")], &[]).contains("support=kitty"));
    assert!(run(&dir, script, &[("TERM", "xterm-kitty"), ("TMUX", "/tmp/tmux-0/default,1,0")], &[]).contains("support=halfblocks"));
    assert!(run(&dir, script, &[("TERM", "xterm-kitty"), ("MATCHBOX_TUI_IMAGES", "halfblocks")], &[]).contains("support=halfblocks"));
    assert!(run(&dir, script, &[("MATCHBOX_TUI_IMAGES", "kitty")], &[]).contains("support=kitty"));
    let _ = fs::remove_dir_all(&dir);
}

fn png_bytes_file(dir: &std::path::Path, name: &str, bytes: &[u8]) {
    fs::write(dir.join(name), bytes).unwrap();
}

#[test]
fn damaged_missing_and_oversized_pictures_return_false_and_the_program_carries_on() {
    let dir = workdir("tui-image-bad");
    png_bytes_file(&dir, "truncated.png", &FOUR_ROWS[..FOUR_ROWS.len() / 2]);
    png_bytes_file(&dir, "empty.png", &[]);
    png_bytes_file(&dir, "text.png", b"definitely not a picture");
    fs::create_dir_all(dir.join("folder")).unwrap();
    let out = run(
        &dir,
        r##"tui = createObject( "rust", "tui.TUI" );
tui.begin_frame();
println( "truncated=" & tui.image( 0, 0, 3, 2, "truncated.png" ) );
println( "empty=" & tui.image( 0, 0, 3, 2, "empty.png" ) );
println( "text=" & tui.image( 0, 0, 3, 2, "text.png" ) );
println( "folder=" & tui.image( 0, 0, 3, 2, "folder" ) );
println( "huge=" & tui.image( 0, 0, 60000, 60000, "four-rows.png" ) );
println( "zero=" & tui.image( 0, 0, 0, 0, "four-rows.png" ) );
println( "again=" & tui.image( 0, 0, 3, 2, "truncated.png" ) );
println( "good=" & tui.image( 0, 0, 2, 2, "four-rows.png" ) );
println( tui.render_to_string( 3, 2 ) );
"##,
        &[],
        &[],
    );
    for line in ["truncated=false", "empty=false", "text=false", "folder=false", "huge=false", "zero=false", "again=false", "good=true"] {
        assert!(out.contains(line), "{line} in {out}");
    }
    assert!(out.contains("\u{2580}\u{2580} "), "{out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_size_and_pixel_limits_can_be_changed_with_environment_variables() {
    let dir = workdir("tui-image-limits");
    let script = r##"tui = createObject( "rust", "tui.TUI" );
tui.begin_frame();
println( "drawn=" & tui.image( 0, 0, 2, 2, "four-rows.png" ) );
"##;
    assert!(run(&dir, script, &[], &[]).contains("drawn=true"));
    assert!(run(&dir, script, &[("MATCHBOX_TUI_IMAGE_MAX_BYTES", "10")], &[]).contains("drawn=false"), "over the byte limit");
    assert!(run(&dir, script, &[("MATCHBOX_TUI_IMAGE_MAX_PIXELS", "7")], &[]).contains("drawn=false"), "2 x 4 = 8 pixels is over a limit of 7");
    assert!(run(&dir, script, &[("MATCHBOX_TUI_IMAGE_MAX_PIXELS", "8")], &[]).contains("drawn=true"), "and exactly at the limit is fine");
    let _ = fs::remove_dir_all(&dir);
}

/// A panic inside the decoder (injected through MATCHBOX_TUI_IMAGE_FAULT, which this test build can recover from) prints nothing,
/// gives false, and the next frame and the next picture draw normally.
#[test]
fn a_decoder_panic_is_silent_gives_false_and_the_next_frame_draws() {
    let dir = workdir("tui-image-fault");
    fs::write(dir.join("other.png"), FOUR_ROWS).unwrap();
    fs::write(dir.join("t.bxs"), r##"tui = createObject( "rust", "tui.TUI" );
tui.begin_frame();
println( "first=" & tui.image( 0, 0, 2, 2, "four-rows.png" ) );
tui.begin_frame();
println( "second=" & tui.image( 0, 0, 2, 2, "other.png" ) );
println( tui.render_to_string( 3, 2 ) );
"##).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_matchbox"))
        .current_dir(&dir)
        .arg("t.bxs")
        .env("MATCHBOX_TUI_IMAGE_FAULT", "decode")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stdout}{stderr}");
    assert!(stdout.contains("first=false"), "{stdout}");
    assert!(stdout.contains("second=true"), "{stdout}");
    assert!(stdout.contains("\u{2580}\u{2580} "), "{stdout}");
    assert!(!stderr.contains("internal error") && !stderr.contains("panicked") && !stderr.contains("injected"), "nothing printed for a caught panic: {stderr}");
    let _ = fs::remove_dir_all(&dir);
}
