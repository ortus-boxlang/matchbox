//! Error reporting behavior for the BoxLang parser (issue #25).
//!
//! These tests verify user-facing error messages: filenames, line:col, and
//! human-readable token names. They use the public `parse` / `parse_bxm`
//! entry points and assert on the rendered error string, not on parser
//! internals.

use matchbox_compiler::parser::{self, ParseError};
use matchbox_compiler::tokenizer::{Span, TokenKind};

// ---------------------------------------------------------------------------
// Slice 1: TokenKind Display is human-readable (no Rust Debug leak).
// ---------------------------------------------------------------------------

#[test]
fn token_kind_display_uses_symbols_and_words() {
    assert_eq!(TokenKind::LeftParen.to_string(), "(");
    assert_eq!(TokenKind::RightParen.to_string(), ")");
    assert_eq!(TokenKind::LeftBrace.to_string(), "{");
    assert_eq!(TokenKind::RightBrace.to_string(), "}");
    assert_eq!(TokenKind::LeftBracket.to_string(), "[");
    assert_eq!(TokenKind::RightBracket.to_string(), "]");
    assert_eq!(TokenKind::Comma.to_string(), ",");
    assert_eq!(TokenKind::Semicolon.to_string(), ";");
    assert_eq!(TokenKind::Identifier.to_string(), "identifier");
    assert_eq!(TokenKind::Number.to_string(), "number");
    assert_eq!(TokenKind::Eof.to_string(), "end of input");
}

// ---------------------------------------------------------------------------
// Slice 2: parse errors include the filename.
// ---------------------------------------------------------------------------

#[test]
fn parse_error_includes_filename() {
    // `function foo(` — missing close paren and body. Should fail to parse.
    let src = "function foo(\n";
    let err = parser::parse(src, Some("app.bxs")).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains("app.bxs"),
        "expected filename in error, got: {rendered}"
    );
}

// ---------------------------------------------------------------------------
// Slice 3: parse errors include the column of the offending token.
// ---------------------------------------------------------------------------

#[test]
fn parse_error_includes_column_of_offending_token() {
    // `if {` — `if` expects `(` but finds `{`. The `{` is at column 4.
    //   1234
    //   if {
    let src = "if {";
    let err = parser::parse(src, Some("app.bxs")).unwrap_err();
    let rendered = err.to_string();
    // The `{` should be reported at its real column 4 on line 1.
    assert!(
        rendered.contains("app.bxs:1:4"),
        "expected location app.bxs:1:4 (the offending `{{`), got: {rendered}"
    );
    assert!(
        !rendered.contains(":1:0"),
        "error must not report col 0, got: {rendered}"
    );
}

// ---------------------------------------------------------------------------
// Slice 4: expect_get errors carry line+col (regression: they used to omit
// the location entirely).
// ---------------------------------------------------------------------------

#[test]
fn expect_get_error_includes_location() {
    // `function (` — `function` keyword consumed, then expect_get(Identifier)
    // for the name but finds `(` at column 10.
    //   123456789
    //   function (
    let src = "function (";
    let err = parser::parse(src, None).unwrap_err();
    let rendered = err.to_string();
    // No filename here, so location should be `line 1:10` form.
    assert!(
        rendered.contains("line 1:10"),
        "expected expect_get error at line 1:10, got: {rendered}"
    );
    assert!(
        rendered.contains("identifier"),
        "expected message to mention 'identifier', got: {rendered}"
    );
}

// ---------------------------------------------------------------------------
// Slice 5: EOF errors carry a meaningful location (end of input, not the
// last token's position).
// ---------------------------------------------------------------------------

#[test]
fn eof_error_carries_end_of_input_location() {
    // `function foo(` then EOF. Parser expects a parameter or `)` but finds
    // end of input. The error should locate at the end of the source (line 1,
    // just past the `(`).
    let src = "function foo(";
    let err = parser::parse(src, Some("app.bxs")).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains("end of input"),
        "expected message to mention 'end of input', got: {rendered}"
    );
    // Location should be on line 1 (the source is a single line), and the
    // column should be past the `(` (col >= 14).
    assert!(
        rendered.contains("app.bxs:1:") && !rendered.contains("app.bxs:1:0"),
        "expected a non-zero col on line 1, got: {rendered}"
    );
    // Span should point at end-of-input (col 14 = length of `function foo(`).
    if let Some(pe) = err.downcast_ref::<ParseError>() {
        assert_eq!(
            pe.span,
            Span {
                start: 13,
                end: 13,
                line: 1,
                col: 14
            },
            "EOF span should point at end of input; got {:?}",
            pe.span
        );
    } else {
        panic!("error should be a ParseError");
    }
}

// ---------------------------------------------------------------------------
// Slice 6: errors render a source snippet with a caret under the offending
// token (the offset target format from the issue).
// ---------------------------------------------------------------------------

#[test]
fn error_renders_source_snippet_with_caret() {
    // `if {` — error at the `{` (col 4). The snippet should show the source
    // line and a caret pointing at column 4.
    let src = "if {";
    let err = parser::parse(src, Some("app.bxs")).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains("if {"),
        "expected the offending source line in the error, got: {rendered}"
    );
    // The caret line should be three spaces then a `^`.
    assert!(
        rendered.contains("   ^"),
        "expected a caret at column 4 (3 spaces of padding), got: {rendered}"
    );
    assert!(
        rendered.contains("--> app.bxs:1:4"),
        "expected the `--> file:line:col` location marker, got: {rendered}"
    );
}

// ---------------------------------------------------------------------------
// Slices 7-10: additional error cases verify message quality across the
// range of parse failures. These don't drive new impl — they lock in the
// user-facing behavior so a regression in any path is caught.
// ---------------------------------------------------------------------------

#[test]
fn error_on_multiline_source_reports_correct_line() {
    // Error on line 3 (the missing `)` after `if (x`).
    //   line 1: var a = 1;
    //   line 2: var b = 2;
    //   line 3: if (x {
    let src = "var a = 1;\nvar b = 2;\nif (x {";
    let err = parser::parse(src, Some("app.bxs")).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains("app.bxs:3:"),
        "expected error on line 3, got: {rendered}"
    );
    // The snippet should show line 3's content and the caret under the `{`.
    assert!(
        rendered.contains("if (x {"),
        "expected the offending line `if (x {{` in the snippet, got: {rendered}"
    );
}

#[test]
fn var_without_assignment_reports_expected_equals() {
    // `var x` then EOF — `var` needs `=` or compound assignment.
    let src = "var x";
    let err = parser::parse(src, Some("app.bxs")).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains('='),
        "expected mention of `=`/assignment, got: {rendered}"
    );
}

#[test]
fn switch_without_case_or_default_reports_error() {
    // `switch (x) { break }` — switch body needs case/default.
    let src = "switch (x) { break; }";
    let err = parser::parse(src, Some("app.bxs")).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains("case") || rendered.contains("default"),
        "expected mention of case/default, got: {rendered}"
    );
}

#[test]
fn malformed_struct_literal_reports_expected_colon_or_equals() {
    // `{ a b }` — struct member needs `:` or `=` after the key.
    let src = "x = { a b };";
    let err = parser::parse(src, Some("app.bxs")).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains(':') || rendered.contains('='),
        "expected mention of `:`/`=` in struct literal, got: {rendered}"
    );
}

// ---------------------------------------------------------------------------
// Slice 11: template parser reports errors with the template's filename and a
// real line number (not line 0).
// ---------------------------------------------------------------------------

#[test]
fn template_script_island_error_includes_filename() {
    // A `.bxm` template with a <bx:script> island containing a syntax error.
    // The island opener is on line 1; the `if` is on template line 2.
    let src = "<bx:script>\nif (x {\n}\n</bx:script>\nhello";
    let err = parser::parse_bxm(src, Some("page.bxm")).unwrap_err();
    let rendered = err.to_string();
    assert!(
        rendered.contains("page.bxm"),
        "expected template filename in error, got: {rendered}"
    );
    // Error should reference line 2 (where `if (x {` lives in the template).
    assert!(
        rendered.contains("page.bxm:2:"),
        "expected error on line 2 of the template, got: {rendered}"
    );
}

#[test]
fn template_output_node_has_real_line_number() {
    // A bare text line in a template should produce a BufferOutput statement
    // whose line number is non-zero (today every template node is line 0).
    let src = "hello\nworld\n";
    let stmts = parser::parse_bxm(src, None).unwrap();
    assert!(!stmts.is_empty(), "expected at least one output statement");
    let any_nonzero = stmts.iter().any(|s| s.line > 0);
    assert!(
        any_nonzero,
        "expected at least one template node with a real line number, got lines: {:?}",
        stmts.iter().map(|s| s.line).collect::<Vec<_>>()
    );
}
