#![cfg(all(not(target_family = "wasm"), not(target_os = "espidf")))]

use matchbox_vm::types::box_string::BoxString;

#[test]
fn deep_concatenations_format_and_serialize_on_a_small_stack() {
    let piece = "é🌍".repeat(4);
    for prepend in [false, true] {
        let mut text = BoxString::new("");
        let part = BoxString::new(&piece);
        for _ in 0..1500 {
            text = if prepend {
                part.concat(&text)
            } else {
                text.concat(&part)
            };
        }
        let expected = piece.repeat(1500);
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(128 * 1024)
                .spawn_scoped(scope, || {
                    assert_eq!(text.to_string(), expected);
                    assert_eq!(
                        serde_json::to_string(&text).unwrap(),
                        serde_json::to_string(&expected).unwrap()
                    );
                })
                .unwrap()
                .join()
                .unwrap();
        });
    }
}

#[test]
fn deep_shared_concatenations_drop_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(|| {
            for prepend in [false, true] {
                let mut text = BoxString::new("");
                let part = BoxString::new("é🌍".repeat(4).as_str());
                for _ in 0..1500 {
                    text = if prepend {
                        part.concat(&text)
                    } else {
                        text.concat(&part)
                    };
                }
                let shared = text.clone();
                drop(text);
                assert_eq!(shared.to_string(), "é🌍".repeat(6000));
                let duplicated = shared.concat(&shared);
                drop(shared);
                assert_eq!(duplicated.to_string(), "é🌍".repeat(12000));
                drop(duplicated);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
