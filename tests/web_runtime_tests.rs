use matchbox_compiler::{compiler::Compiler, parser};
use matchbox_vm::types::{BxNativeFunction, BxVM, BxValue, NativeFutureValue};
use matchbox_vm::vm::VM;
use std::collections::HashMap;
use std::fs;
use std::time::Duration;
use tempfile::tempdir;

#[test]
fn test_bxm_transpilation() {
    let bxm_source = r#"
        <bx:set x = 10>
        <bx:if condition="x == 10">
            <bx:output>Value is #x#</bx:output>
        </bx:if>
    "#;

    // Verify template parser handles basic BXM
    let result = matchbox_compiler::parser::parse_bxm(bxm_source, None);
    assert!(
        result.is_ok(),
        "Template parser should handle basic BXM: {:?}",
        result.err()
    );
}

fn compile_source(path: &str, source: &str) -> matchbox_vm::vm::chunk::Chunk {
    let ast = parser::parse(source, Some(path)).unwrap();
    let mut compiler = Compiler::new(path);
    compiler.compile(&ast, source).unwrap()
}

#[test]
fn test_vm_output_buffering() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = "writeOutput('Hello ', 'World'); println('!'); print('MatchBox');";
    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "Hello World!\nMatchBox");
}

#[test]
fn test_include_executes_nested_relative_files() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    let deps = root.join("deps");
    fs::create_dir_all(&deps).unwrap();

    let grandchild = deps.join("grandchild.bxs");
    let child = deps.join("child.bxs");
    let main = root.join("main.bxs");

    fs::write(&grandchild, r#"var includedValue = "included";"#).unwrap();
    fs::write(&child, r#"include "grandchild";"#).unwrap();
    fs::write(
        &main,
        r#"
            include "deps/child";
            writeOutput(includedValue);
        "#,
    )
    .unwrap();

    let mut bifs = HashMap::new();
    bifs.insert(
        "include".to_string(),
        matchbox::include_bif as BxNativeFunction,
    );
    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let main_path = main.to_str().unwrap();
    let source = fs::read_to_string(&main).unwrap();
    let chunk = compile_source(main_path, &source);

    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap().trim(), "included");
}

#[test]
fn test_rethrow_preserves_active_exception() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        try {
            try {
                throw "inner error";
            } catch (e) {
                rethrow;
            }
        } catch (e) {
            println(e.message);
        }
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "inner error\n");
}

#[test]
fn test_rethrow_outside_catch_is_rejected() {
    let source = "rethrow;";
    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let err = compiler.compile(&ast, source).unwrap_err();

    assert!(
        err.to_string().contains("inside a catch block"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_access_modifier_enforcement() {
    let cases = [
        (
            r#"
                abstract class Base {
                }
                new Base();
            "#,
            "abstract class",
        ),
        (
            r#"
                final class Base {
                }
                class Child extends="Base" {
                }
            "#,
            "final",
        ),
        (
            r#"
                class Base {
                    final function getName() {
                        return "base";
                    }
                }
                class Child extends="Base" {
                    function getName() {
                        return "child";
                    }
                }
            "#,
            "final method",
        ),
        (
            r#"
                class Sample {
                    static function getValue() {
                        return this;
                    }
                }
            "#,
            "static function",
        ),
    ];

    for (source, needle) in cases {
        let ast = parser::parse(source, Some("test")).unwrap();
        let mut compiler = Compiler::new("test");
        let err = compiler.compile(&ast, source).unwrap_err().to_string();
        assert!(
            err.to_lowercase().contains(&needle.to_lowercase()),
            "expected `{err}` to contain `{needle}`"
        );
    }
}

#[test]
fn test_special_operators_evaluate_each_operand_once() {
    let mut vm = VM::new();

    let source = r#"
        var c = 0;

        function bump() {
            c = c + 1;
            return c;
        }

        function typeName() {
            c = c + 1;
            return "numeric";
        }

        var rangeVal = bump()..bump();
        var containsVal = bump() contains bump();
        var instanceofVal = bump() instanceof typeName();
        var castasVal = bump() castas typeName();

        if (len(rangeVal) != 2) { throw "range result wrong"; }
        if (containsVal != false) { throw "contains result wrong"; }
        if (instanceofVal != true) { throw "instanceof result wrong"; }
        if (castasVal != 7) { throw "castas result wrong"; }
        if (c != 8) { throw "special operator operands evaluated wrong number of times: " & c; }
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();
}

#[test]
fn test_case_insensitive_keywords_and_word_operators() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        var out = "";

        IF (TRUE AND 3 GT 2) {
            out = out & "A";
        }

        if (1 LTE 1 or 5 NEQ 5) {
            out = out & "B";
        }

        if (false OR 2 lt 3) {
            out = out & "C";
        }

        writeOutput(out);
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ABC");
}

#[test]
fn test_phrase_word_operators() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        var out = "";

        if ("hello" not contains "z") {
            out = out & "A";
        }

        if ("hello" does not contain "z") {
            out = out & "B";
        }

        if (1 less than 2) {
            out = out & "C";
        }

        if (2 greater than or equal to 2) {
            out = out & "D";
        }

        if (1 is not 2) {
            out = out & "E";
        }

        writeOutput(out);
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ABCDE");
}

#[test]
fn test_string_helpers() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        s = "BoxLang is great";

        if (find("Lang", s) != 4) { throw "find failed"; }
        if (findNoCase("lang", s) != 4) { throw "findNoCase failed"; }
        if (find("Lang", s, 2) != 4) { throw "find start failed"; }
        if (find("zzz", s) != 0) { throw "find not-found failed"; }

        if (s.find("Lang") != 4) { throw "member find failed"; }
        if (s.findNoCase("lang") != 4) { throw "member findNoCase failed"; }

        if ("abcdef".left(3) != "abc") { throw "left failed"; }
        if ("abcdef".right(3) != "def") { throw "right failed"; }
        if ("abcdef".mid(2, 3) != "bcd") { throw "mid failed"; }
        if ("abcdef".reverse() != "fedcba") { throw "reverse failed"; }
        if ("  hi  ".trim() != "hi") { throw "trim failed"; }
        if (spanExcluding("MyString", "inS") != "My") { throw "spanExcluding failed"; }
        if ("Highway Star".spanIncluding("High") != "High") { throw "spanIncluding failed"; }

        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_regex_match_helpers() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        case_sensitive = reMatch("[abc]", "abc");
        case_insensitive = reMatchNoCase("[abc]", "AbC");

        if (len(case_sensitive) != 3) { throw "reMatch length failed"; }
        if (case_sensitive[1] != "a" || case_sensitive[2] != "b" || case_sensitive[3] != "c") {
            throw "reMatch content failed";
        }

        if (len(case_insensitive) != 3) { throw "reMatchNoCase length failed"; }
        if (case_insensitive[1] != "A" || case_insensitive[2] != "b" || case_insensitive[3] != "C") {
            throw "reMatchNoCase content failed";
        }

        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_regex_find_helpers() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        one = reFind("(1)[2-3]", "test 123 test 123!", 1, true, "one");
        all = reFind("(1)[2-3]", "test 123 test 123!", 1, false, "all");
        no_case = reFindNoCase("test", "THIS IS A TEST", 1, false, "one");

        if (one.len[1] != 2 || one.len[2] != 1) { throw "reFind subexpression lengths failed"; }
        if (one.match[1] != "12" || one.match[2] != "1") { throw "reFind subexpression matches failed"; }
        if (one.pos[1] != 6 || one.pos[2] != 6) { throw "reFind subexpression positions failed"; }

        if (len(all) != 2 || all[1] != 6 || all[2] != 15) { throw "reFind all positions failed"; }
        if (no_case != 11) { throw "reFindNoCase failed"; }

        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_regex_replace_helpers() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        all = reReplace("foo BAR foo", "foo", "qux", "all");
        one = reReplace("foo BAR foo", "foo", "qux", "one");
        no_case = "foo BAR foo".reReplaceNoCase("foo", "qux", "all");

        if (all != "qux BAR qux") { throw "reReplace all failed"; }
        if (one != "qux BAR foo") { throw "reReplace one failed"; }
        if (no_case != "qux BAR qux") { throw "reReplaceNoCase member failed"; }

        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_json_helpers() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        payload = {
            name: "MatchBox",
            values: [1, 2, null, true, false]
        };

        json = serializeJSON(payload);
        member_json = payload.toJSON();
        roundtrip = deserializeJSON(json);
        member_roundtrip = json.fromJSON();

        if (!isJSON(json)) { throw "isJSON failed"; }
        if (isJSON("not json")) { throw "isJSON false-positive"; }
        if (json != '{"name":"MatchBox","values":[1,2,null,true,false]}') {
            throw "serializeJSON failed: " & json;
        }
        if (member_json != json) { throw "member toJSON failed"; }
        if (roundtrip.name != "MatchBox") { throw "deserializeJSON struct failed"; }
        if (member_roundtrip.name != "MatchBox") { throw "member fromJSON struct failed"; }
        if (roundtrip.values[1] != 1 || roundtrip.values[3] != null || roundtrip.values[5] != false) {
            throw "deserializeJSON array failed";
        }

        if (serializeJSON(null) != "null") { throw "serializeJSON null failed"; }

        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_utility_helpers() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        uuid = createUUID();
        if (len(uuid) != 36) { throw "createUUID length failed"; }
        if (uuid != uCase(uuid)) { throw "createUUID case failed"; }

        ref = {
            nested: [1, { value: "orig" }],
            stamp: now(),
            raw: bytesNew([1, 2, 3])
        };

        dup = duplicate(ref);
        memberDup = ref.duplicate();

        dup.nested[2].value = "changed";
        memberDup.nested[2].value = "member";

        if (ref.nested[2].value != "orig") { throw "duplicate deep copy failed"; }
        if (dup.nested[2].value != "changed") { throw "duplicate mutation failed"; }
        if (memberDup.nested[2].value != "member") { throw "member duplicate mutation failed"; }
        if (!isArray(ref.nested)) { throw "isArray failed"; }
        if (!isStruct(ref)) { throw "isStruct failed"; }
        if (!isDate(ref.stamp)) { throw "isDate failed"; }
        if (!isBinary(ref.raw)) { throw "isBinary failed"; }
        if (!isBoolean(true) || !isBoolean("yes") || isBoolean("randomstring")) { throw "isBoolean failed"; }
        if (!isString("abc") || isString(ref)) { throw "isString failed"; }
        if (isObject(ref) || isObject(ref.nested) || isObject(ref.stamp)) { throw "isObject failed"; }
        if (!isSimpleValue(ref.stamp)) { throw "isSimpleValue date failed"; }

        sleep(1);
        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_crypto_helpers() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        input = "Hello World";

        if (hash(input) != "b10a8db164e0754105b7a99be72e3fe5") { throw "default hash failed"; }
        if (hash(input, "MD5") != "b10a8db164e0754105b7a99be72e3fe5") { throw "md5 hash failed"; }
        if (hash(input, "SHA") != "0a4d55a8d778e5022fab701977c5d840bbc486d0") { throw "sha1 hash failed"; }
        if (hash(input, "SHA-256") != "a591a6d40bf420404a011733cfb7b190d62c65bf0bcda32b57b277d9ad9f146e") { throw "sha256 hash failed"; }
        if (hash(input, "SHA-384") != "99514329186b2f6ae4a1329e7ee6c610a729636335174ac6b740f9028396fcc803d0e93863a7c3d90f86beee782f4f3f") { throw "sha384 hash failed"; }
        if (hash(input, "SHA-512") != "2c74fd17edafd80e8447b0d46741ee243b7eb74dd2149a0ab1b9246fb30382f27e853d8585719e0e67cbda0daa8f51671064615d645ae27acb15bfb1447f459b") { throw "sha512 hash failed"; }

        if (hash("foo", "QUICK") != "4d780c14822d4653") { throw "quick hash failed"; }
        if (hash(input, "bxmX_COMPAT") != hash(input, "MD5")) { throw "compat hash failed"; }
        if (hash(bytesNew([72, 101, 108, 108, 111, 32, 87, 111, 114, 108, 100])) != hash(input)) { throw "byte hash failed"; }
        if (hash("hello world", "md5", "utf-8", 2) != "241d8a27c836427bd7f04461b60e7359") { throw "hash iterations failed"; }

        if (hmac("Hmac me baby!", "foo") != "48bfb8004f92d6c9e9eac9728c5d919c") { throw "hmac md5 failed"; }
        if ("Hmac me baby!".hmac("foo") != "48bfb8004f92d6c9e9eac9728c5d919c") { throw "member hmac failed"; }
        if (lcase(hmac(
            "foo",
            bytesNew([15, 107, 76, 217, 13, 96, 99, 125, 52, 165, 71, 238, 181, 130, 111, 168, 231, 31, 85, 175, 207, 166, 65, 15, 187, 54, 5, 66, 136, 183, 100, 205]),
            "HMACSHA256",
            "UTF-8"
        )) != "3483aeb10dab06f29b8037366fdf819b5c0c09a99213cae50a3474d4edbbd400") {
            throw "hmac sha256 failed";
        }

        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_list_helpers() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        list = "a,b,c";
        if (listLen(list) != 3) { throw "listLen failed"; }
        if (listGetAt(list, 2) != "b") { throw "listGetAt failed"; }
        if (listFirst(list) != "a") { throw "listFirst failed"; }
        if (listLast(list) != "c") { throw "listLast failed"; }
        if (listAppend(list, "d") != "a,b,c,d") { throw "listAppend failed"; }
        if (listRest(list) != "b,c") { throw "listRest failed"; }
        if (listDeleteAt(list, 2) != "a,c") { throw "listDeleteAt failed"; }
        if (listFind(list, "c") != 3) { throw "listFind failed"; }
        if (listFindNoCase("A,B,C", "b") != 2) { throw "listFindNoCase failed"; }

        if (listAppend("1|2|3", "4", "|") != "1|2|3|4") { throw "custom append failed"; }
        if (listRest("1-and-2-and-3", "-and-", true, true) != "2-and-3") { throw "multi-char rest failed"; }
        if (listDeleteAt("1-and-2-and-3", 2, "-and-", true, true) != "1-and-3") { throw "multi-char delete failed"; }
        if (listSort("b,d,c,a", "text", "asc") != "a,b,c,d") { throw "listSort failed"; }

        if (listSort("b,d,c,a", "textnocase", "desc") != "d,c,b,a") { throw "listSort textnocase failed"; }

        if (list.listLen() != 3) { throw "member listLen failed"; }
        if (list.listGetAt(1) != "a") { throw "member listGetAt failed"; }
        if (list.listAppend("d") != "a,b,c,d") { throw "member listAppend failed"; }
        if (list.listFirst() != "a") { throw "member listFirst failed"; }
        if (list.listLast() != "c") { throw "member listLast failed"; }
        if (list.listRest() != "b,c") { throw "member listRest failed"; }
        if (list.listDeleteAt(2) != "a,c") { throw "member listDeleteAt failed"; }
        if (list.listFind("c") != 3) { throw "member listFind failed"; }
        if (list.listFindNoCase("B") != 2) { throw "member listFindNoCase failed"; }
        if (list.listSort("text", "asc") != "a,b,c") { throw "member listSort failed"; }

        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_math_helpers() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        randomize(12345);
        first = rand();
        randomize(12345);
        second = rand();

        if (first != second) { throw "randomize failed"; }
        if (first < 0 || first >= 1) { throw "rand range failed"; }

        if (round(1.4) != 1) { throw "round failed"; }
        if (floor(1.8) != 1) { throw "floor failed"; }
        if (ceiling(1.2) != 2) { throw "ceiling failed"; }
        if (log(1) != 0) { throw "log failed"; }
        if (log10(10) != 1) { throw "log10 failed"; }
        if (exp(0) != 1) { throw "exp failed"; }
        if (sin(0) != 0) { throw "sin failed"; }
        if (cos(0) != 1) { throw "cos failed"; }
        if (tan(0) != 0) { throw "tan failed"; }
        if (asin(0) != 0) { throw "asin failed"; }
        if (acos(1) != 0) { throw "acos failed"; }
        if (atan(0) != 0) { throw "atan failed"; }
        if (atan2(0, 1) != 0) { throw "atan2 failed"; }
        if (pi() < 3.14 || pi() > 3.15) { throw "pi failed"; }

        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_date_time_and_bifs() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        d = createDate(2024, 1, 1);
        dt = createDateTime(2024, 1, 1, 12, 30, 45, 500, "UTC");
        parsed = parseDateTime("2024-01-01T12:30:45.500Z");
        base = parseDateTime("2024-01-01T12:30:45.000Z");
        next = dateAdd("d", 1, d);
        shifted = dateAdd("s", 0.5, base);

        if (dateFormat(d) != "01-Jan-24") {
            throw "dateFormat failed: " & dateFormat(d);
        }
        if (dateTimeFormat(dt, "yyyy-MM-dd'T'HH:mm:ss.SSSX", "UTC") != "2024-01-01T12:30:45.500Z") {
            throw "dateTimeFormat failed";
        }
        if (parsed != dt) {
            throw "parseDateTime failed";
        }
        if (dateDiff("d", next, d) != -1) {
            throw "dateDiff days failed";
        }
        if (dateTimeFormat(shifted, "yyyy-MM-dd'T'HH:mm:ss.SSSX", "UTC") != "2024-01-01T12:30:46.000Z") {
            throw "fractional dateAdd failed";
        }
        if (!(d < next)) {
            throw "datetime comparison failed";
        }

        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_function_and_struct_spread() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        function sum(a, b, c) {
            return a + b + c;
        }

        var args = [1, 2];
        if (sum(...args, 3) != 6) {
            throw "function spread failed";
        }

        var base = { b: 2, c: 3 };
        var merged = { a: 1, ...base, d: 4 };
        if (merged.a != 1 || merged.b != 2 || merged.c != 3 || merged.d != 4) {
            throw "struct spread failed";
        }

        writeOutput("ok");
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_array_and_struct_helpers() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        nums = [1, 2, 3];
        nums.resize(5);
        ArraySwap(nums, 1, 3);
        if (len(nums) != 5) {
            throw "arrayResize failed";
        }
        if (nums[1] != 3 || nums[3] != 1 || nums[4] != null || nums[5] != null) {
            throw "arraySwap or arrayResize values wrong";
        }

        data = { foo: "bar" };
        if (structFind(data, "foo") != "bar") {
            throw "structFind failed";
        }
        if (structFind(data, "FOO") != "bar") {
            throw "structFind case-insensitive failed";
        }
        if (!structIsEmpty(structNew())) {
            throw "structIsEmpty failed";
        }
        if (data.find("foo") != "bar") {
            throw "struct member find failed";
        }

        writeOutput("ok");
    "#;

    let chunk = compile_source("test", source);
    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "ok");
}

#[test]
fn test_destructuring_evaluates_source_once_and_handles_arrays() {
    let mut vm = VM::new();

    let source = r#"
        var calls = 0;

        function makeStruct() {
            calls = calls + 1;
            return { a: 1, b: 2 };
        }

        function makeArray() {
            calls = calls + 1;
            return [10, 20];
        }

        var first = 0;
        var second = 0;
        { a: first, b: second } = makeStruct();
        if (first != 1 || second != 2) {
            throw "object destructuring failed";
        }

        var left = 0;
        var right = 0;
        [left, right] = makeArray();
        if (left != 10 || right != 20) {
            throw "array destructuring failed";
        }

        if (calls != 2) {
            throw "destructuring source evaluated wrong number of times: " & calls;
        }
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();
}

#[test]
fn test_invalid_spread_values_error() {
    let mut vm = VM::new();
    let source = r#"
        var bad = [...1];
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    let err = vm.interpret(chunk).unwrap_err();
    let err_text = err.to_string();
    assert!(
        err_text.contains("Cannot spread value of type"),
        "unexpected error: {err_text}"
    );
}

#[test]
fn test_weak_typing_addition() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    // Test: string "10" + number 5 = 15
    // Test: string "1.5" + string "2.5" = 4.0
    // Test: string "Hello" + 5 = "Hello5" (fallback to concat)
    let source = r#"
        writeOutput("10" + 5);
        writeOutput("|");
        writeOutput("1.5" + "2.5");
        writeOutput("|");
        writeOutput("Hello" + 5);
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "15|4|Hello5");
}

#[test]
fn test_script_variables_scope_is_shared_with_functions() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        variables.data = { value : 1 };

        function updateData() {
            variables.data.value = variables.data.value + 41;
        }

        updateData();
        writeOutput(variables.data.value);
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    assert_eq!(vm.output_buffer.unwrap(), "42");
}

#[cfg(feature = "debugger")]
#[test]
fn test_debugger_steps_source_lines_and_reads_variables_data() {
    use matchbox_vm::vm::DebugStepStatus;

    let mut vm = VM::new();
    let source = r#"
        variables.data = { value : 1 };
        variables.data = { value : 2 };
        variables.data = { value : 3 };
    "#;

    let ast = parser::parse(source, Some("debug_test")).unwrap();
    let mut compiler = Compiler::new("debug_test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.start_debug_chunk(chunk).unwrap();

    let first = vm.debug_step_source_line(2000, Some("variables.data"));
    assert_eq!(first.status, DebugStepStatus::Paused);
    assert_eq!(first.value.unwrap()["value"], serde_json::json!(1.0));

    let second = vm.debug_step_source_line(2000, Some("variables.data"));
    assert_eq!(second.status, DebugStepStatus::Paused);
    assert_eq!(second.value.unwrap()["value"], serde_json::json!(2.0));

    let third = vm.debug_step_source_line(2000, Some("variables.data"));
    assert!(matches!(
        third.status,
        DebugStepStatus::Paused | DebugStepStatus::Completed
    ));
}

#[cfg(feature = "debugger")]
#[test]
fn test_debugger_instruction_budget_is_resumable() {
    use matchbox_vm::vm::DebugStepStatus;

    let mut vm = VM::new();
    let source = r#"
        variables.data = { value : 0 };
        for (var i = 0; i < 10; i++) {
            variables.data.value = variables.data.value + 1;
        }
    "#;

    let ast = parser::parse(source, Some("debug_budget")).unwrap();
    let mut compiler = Compiler::new("debug_budget");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.start_debug_chunk(chunk).unwrap();

    let first = vm.debug_step_source_line(1, Some("variables.data"));
    assert_eq!(first.status, DebugStepStatus::BudgetExhausted);
    assert_eq!(first.instructions, 1);

    let second = vm.debug_step_source_line(2000, Some("variables.data"));
    assert!(matches!(
        second.status,
        DebugStepStatus::Paused | DebugStepStatus::Completed
    ));
}

#[test]
fn test_nested_bxm_interpolation() {
    // Verify template parser handles interpolation without error
    let bxm_source = r#"<bx:output>#1 + 1# is #2# and ## is literal</bx:output>"#;
    let result = parser::parse_bxm(bxm_source, Some("test"));
    assert!(
        result.is_ok(),
        "Template parser should handle interpolation BXM: {:?}",
        result.err()
    );
}

#[test]
fn test_bxm_script_island_and_output_semantics() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        <bx:script>
            var name = "World";
        </bx:script>
        <bx:output>Hello #name#! ## #name#</bx:output>
    "#;

    let ast = parser::parse_bxm(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    assert_eq!(
        vm.output_buffer.unwrap(),
        "\n        \n        Hello World! # World\n    "
    );
}

#[test]
fn test_quoted_struct_literal_keys_preserve_original_case() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    let source = r#"
        options = {
            "acceptAllDevices": true,
            "optionalServices": ["service-a", "service-b"]
        };
        keys = structKeyArray(options);
        writeOutput(keys[1] & "|" & keys[2]);
    "#;

    let ast = parser::parse(source, None).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "acceptAllDevices|optionalServices");
}

fn rejected_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let future = vm.future_new();
    let err_id = vm.struct_new();
    let msg_id = vm.string_new("boom".to_string());
    vm.struct_set(err_id, "message", BxValue::new_ptr(msg_id));
    vm.future_reject(future, BxValue::new_ptr(err_id))?;
    Ok(future)
}

fn resolved_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let future = vm.future_new();
    let value_id = vm.string_new("done".to_string());
    vm.future_resolve(future, BxValue::new_ptr(value_id))?;
    Ok(future)
}

fn queued_resolved_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let future = vm.future_new();
    let value_id = vm.string_new("queued".to_string());
    vm.future_schedule_resolve(future, BxValue::new_ptr(value_id))?;
    Ok(future)
}

fn queued_rejected_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let future = vm.future_new();
    let err_id = vm.struct_new();
    let msg_id = vm.string_new("queued-boom".to_string());
    vm.struct_set(err_id, "message", BxValue::new_ptr(msg_id));
    vm.future_schedule_reject(future, BxValue::new_ptr(err_id))?;
    Ok(future)
}

fn threaded_resolved_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let handle = vm.native_future_new();
    let future = handle.future();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5));
        let _ = handle.resolve(NativeFutureValue::String("threaded".to_string()));
    });
    Ok(future)
}

fn threaded_rejected_future(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let handle = vm.native_future_new();
    let future = handle.future();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(5));
        let _ = handle.reject(NativeFutureValue::Error {
            message: "threaded-boom".to_string(),
        });
    });
    Ok(future)
}

#[test]
fn test_native_future_rejection_propagates_value_to_catch() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "rejectedfuture".to_string(),
        rejected_future as BxNativeFunction,
    );

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        try {
            rejectedFuture().get();
            throw "expected rejection";
        } catch (err) {
            if (err.message != "boom") {
                throw "unexpected rejection payload: " & err.message;
            }
            writeOutput("ok");
        }
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "ok");
}

#[test]
fn test_native_future_resolution_returns_value_from_get() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "resolvedfuture".to_string(),
        resolved_future as BxNativeFunction,
    );

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        writeOutput(resolvedFuture().get());
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "done");
}

#[test]
fn test_queued_future_resolution_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "queuedresolvedfuture".to_string(),
        queued_resolved_future as BxNativeFunction,
    );

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        writeOutput(queuedResolvedFuture().get());
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "queued");
}

#[test]
fn test_queued_future_rejection_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "queuedrejectedfuture".to_string(),
        queued_rejected_future as BxNativeFunction,
    );

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        try {
            queuedRejectedFuture().get();
            throw "expected queued rejection";
        } catch (err) {
            writeOutput(err.message);
        }
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "queued-boom");
}

#[test]
fn test_threaded_future_resolution_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "threadedresolvedfuture".to_string(),
        threaded_resolved_future as BxNativeFunction,
    );

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        writeOutput(threadedResolvedFuture().get());
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "threaded");
}

#[test]
fn test_threaded_future_rejection_is_applied_by_scheduler() {
    let mut bifs = HashMap::new();
    bifs.insert(
        "threadedrejectedfuture".to_string(),
        threaded_rejected_future as BxNativeFunction,
    );

    let mut vm = VM::new_with_bifs(bifs, HashMap::new());
    vm.output_buffer = Some(String::new());

    let source = r#"
        try {
            threadedRejectedFuture().get();
            throw "expected threaded rejection";
        } catch (err) {
            writeOutput(err.message);
        }
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "threaded-boom");
}

#[test]
fn test_js_import_binds_to_global() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    // Set up a mock js global so the import has something to resolve.
    let setup = r#"
        class MockConsole {
            function log(msg) {
                writeOutput(msg);
            }
        }

        js = {
            console: new MockConsole(),
            window: {
                document: {
                    title: "Mock Title"
                }
            }
        };
    "#;
    let setup_ast = parser::parse(setup, Some("setup")).unwrap();
    let mut setup_compiler = Compiler::new("setup");
    let setup_chunk = setup_compiler.compile(&setup_ast, setup).unwrap();
    vm.interpret(setup_chunk).unwrap();

    let source = r#"
        import js:console;
        console.log("hello from console");

        import js:window.document as doc;
        writeOutput("|");
        writeOutput(doc.title);
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "hello from console|Mock Title");
}

#[test]
fn test_js_import_constructor_native_mock() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    // Set up a mock js global with a simple value.
    let setup = r#"
        js = {
            MyMockCtor: "hello"
        };
    "#;

    let source = r#"
        import js:MyMockCtor;
        writeOutput(MyMockCtor);
    "#;
    let setup_ast = parser::parse(setup, Some("setup")).unwrap();
    let mut setup_compiler = Compiler::new("setup");
    let setup_chunk = setup_compiler.compile(&setup_ast, setup).unwrap();
    vm.interpret(setup_chunk).unwrap();

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "hello");
}

#[test]
fn test_js_import_constructor_inside_class_native() {
    let mut vm = VM::new();
    vm.output_buffer = Some(String::new());

    // Mock the js global with a BoxLang class (simulates a JS constructor).
    // `this.encoding` is set in the class body so it runs in the auto-generated
    // constructor, matching how JS constructors work without a separate init().
    let setup = r#"
        class MockTextEncoder {
            this.encoding = "utf-8";
        }
        js = {
            TextEncoder: MockTextEncoder
        };
    "#;
    let setup_ast = parser::parse(setup, Some("setup")).unwrap();
    let mut setup_compiler = Compiler::new("setup");
    let setup_chunk = setup_compiler.compile(&setup_ast, setup).unwrap();
    vm.interpret(setup_chunk).unwrap();

    let source = r#"
        import js:TextEncoder;

        class Writer {
            function init() {
                variables.encoder = new TextEncoder();
                return this;
            }

            function getEncoding() {
                return variables.encoder.encoding;
            }
        }

        w = new Writer();
        writeOutput(w.getEncoding());
    "#;

    let ast = parser::parse(source, Some("test")).unwrap();
    let mut compiler = Compiler::new("test");
    let chunk = compiler.compile(&ast, source).unwrap();

    vm.interpret(chunk).unwrap();

    let output = vm.output_buffer.unwrap();
    assert_eq!(output, "utf-8");
}

#[test]
fn test_cross_file_js_import_propagation() {
    use std::env;
    use std::fs;
    use std::path::Path;

    let tmp_dir = Path::new("/tmp/cross_file_test_dir");
    fs::remove_dir_all(tmp_dir).ok();
    fs::create_dir_all(tmp_dir).ok();
    fs::create_dir_all(tmp_dir.join("modules/tspl/models")).ok();

    fs::write(
        tmp_dir.join("modules/tspl/models/Writer.bx"),
        r#"
import js:TextEncoder;
class Writer {
    function init() {
        variables.encoder = new TextEncoder();
        return this;
    }
}
"#,
    )
    .unwrap();

    fs::write(
        tmp_dir.join("test.bxs"),
        r#"
class MockTextEncoder {
    this.encoding = "utf-8";
}
js = { TextEncoder: MockTextEncoder };
import modules.tspl.models.Writer;
writer = new Writer();
writeOutput("PASS");
"#,
    )
    .unwrap();

    let orig_dir = env::current_dir().unwrap();
    env::set_current_dir(tmp_dir).unwrap();

    let source = fs::read_to_string(tmp_dir.join("test.bxs")).unwrap();
    let ast = matchbox_compiler::parser::parse(&source, Some("test")).unwrap();
    let mut compiler = matchbox_compiler::compiler::Compiler::new("test");
    let chunk = compiler.compile(&ast, &source).unwrap();

    env::set_current_dir(orig_dir).unwrap();

    let mut vm = matchbox_vm::vm::VM::new();
    vm.output_buffer = Some(String::new());
    vm.interpret(chunk).unwrap();
    assert_eq!(vm.output_buffer.unwrap(), "PASS");
}
