//! A host calling a BoxLang function that blocks on a future (`.get()`) must get the function's return value,
//! not null. The embedder path is `VM::call_function_value`.

use matchbox_vm::{types::{BxValue, BxVM}, vm::VM};

const SCRIPT: &str = r#"
function fetchLength( url ) {
    var r = jsonDeserialize( http( { url: url, timeout: 10, noProxy: true } ).get() );
    return r.status;
}
"#;

/// Serves `rounds` plain HTTP responses on a local port after a short delay, so the future is still pending
/// when the script reaches `.get()`.
fn serve(rounds: usize) -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for _ in 0..rounds {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let _ = sock.read(&mut buf);
            std::thread::sleep(std::time::Duration::from_millis(15));
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    port
}

fn compile(src: &str) -> matchbox_vm::Chunk {
    let ast = matchbox_compiler::parser::parse(src, Some("embed.bxs")).expect("parse");
    let mut chunk = matchbox_compiler::compile_with_treeshaking("embed.bxs", &ast, src, Vec::new(), false, false, false, &[])
        .expect("compile");
    chunk.reconstruct_functions();
    chunk
}

#[test]
fn call_function_value_returns_the_value_after_a_blocking_get() {
    // The bug depended on scheduler timing, so repeat.
    let rounds = 25;
    let port = serve(rounds);
    for round in 0..rounds {
        let mut vm = VM::new();
        vm.interpret(compile(SCRIPT)).expect("define");
        let f = vm.get_global("fetchLength").expect("function");
        let url = BxValue::new_ptr(vm.string_new(format!("http://127.0.0.1:{port}/")));
        let out = vm.call_function_value(f, vec![url], None).expect("call");
        assert_eq!(vm.to_string(out), "200", "round {round}");
    }
}
