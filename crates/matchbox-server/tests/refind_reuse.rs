use matchbox_server::webroot_core::{
    EmbeddedAssetStore, WebrootConfig, WebrootEngine, WebrootRequest,
};
use std::collections::HashMap;

fn engine() -> WebrootEngine<EmbeddedAssetStore> {
    WebrootEngine::new(
        EmbeddedAssetStore::new(HashMap::from([("index.bxs".into(), br#"
            try {
                if (url.nocase == "true") {
                    result = reFindNoCase(url.pattern, url.text, val(url.start), url.subs == "true", url.scope);
                } else {
                    result = reFind(url.pattern, url.text, val(url.start), url.subs == "true", url.scope);
                }
                writeOutput(jsonSerialize(result));
            } catch (any e) { writeOutput(jsonSerialize({error: e.message})); }
        "#.to_vec())])),
        WebrootConfig::default(),
    )
}

fn find(
    e: &WebrootEngine<EmbeddedAssetStore>,
    pattern: &str,
    text: &str,
    nocase: bool,
    start: i32,
    subs: bool,
    scope: &str,
) -> serde_json::Value {
    let response = e
        .handle(WebrootRequest {
            method: "GET".into(),
            path: "/index.bxs".into(),
            query: HashMap::from([
                ("pattern".into(), pattern.into()),
                ("text".into(), text.into()),
                ("nocase".into(), nocase.to_string()),
                ("start".into(), start.to_string()),
                ("subs".into(), subs.to_string()),
                ("scope".into(), scope.into()),
            ]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(response.status, 200);
    serde_json::from_slice(&response.body).unwrap()
}

#[test]
fn refind_repeated_requests_preserve_patterns_flags_text_and_captures() {
    use serde_json::json;
    let first = engine();
    let second = engine();
    for _ in 0..3 {
        for e in [&first, &second] {
            assert_eq!(find(e, "Ab", "xAB", false, 1, false, "one"), json!(0));
            assert_eq!(find(e, "Ab", "xAB", true, 1, false, "one"), json!(2));
            assert_eq!(find(e, "Ab", "Ab", false, 1, false, "one"), json!(1));
            assert_eq!(find(e, "é", "é猫é", false, 2, false, "one"), json!(3));
            assert_eq!(find(e, "é", "xÉ", true, 1, false, "one"), json!(2));
            assert_eq!(find(e, "a", "abc", false, -2, false, "one"), json!(1));
            assert_eq!(find(e, "a", "abc", false, 99, false, "one"), json!(0));
            assert_eq!(find(e, "", "abc", false, 99, false, "one"), json!(99));
            assert_eq!(find(e, "a.b", "a\nb", false, 1, false, "one"), json!(1));
            assert_eq!(
                find(e, "a{word}", "xa{word}", false, 1, false, "one"),
                json!(2)
            );
            assert_eq!(find(e, "(?i)Ab", "xAB", false, 1, false, "one"), json!(2));
            assert_eq!(
                find(e, "[[:lower:]]", "ABC", false, 1, false, "one"),
                json!(0)
            );
            assert_eq!(
                find(e, "[[:lower:]]", "ABC", true, 1, false, "one"),
                json!(1)
            );
            assert_eq!(
                find(e, "[[:digit:]]+", "x42", false, 1, false, "one"),
                json!(2)
            );
            assert_eq!(
                find(e, "(a)(b)?", "xaba", false, 1, false, "ALL"),
                json!([2, 4])
            );
            assert_eq!(
                find(e, "(a)(b)?", "xaba", false, 1, true, "all"),
                json!([
                    {"len":[2,1,1],"match":["ab","a","b"],"pos":[2,2,3]},
                    {"len":[1,1,0],"match":["a","a",""],"pos":[4,4,0]}
                ])
            );
            assert_eq!(
                find(e, "(a)", "bb", false, 1, true, "one"),
                json!({"len":[0],"match":[""],"pos":[0]})
            );
        }
    }
}

#[test]
fn refind_independent_workers_do_not_share_request_text_or_captures() {
    let workers: Vec<_> = (0..4)
        .map(|n| {
            std::thread::spawn(move || {
                let e = engine();
                for _ in 0..20 {
                    let text = format!("worker{n}");
                    assert_eq!(
                        find(&e, "worker([0-9])", &text, false, 1, true, "one"),
                        serde_json::json!({"len":[7,1], "match":[text,n.to_string()], "pos":[1,7]})
                    );
                    assert_eq!(
                        find(&e, "worker([0-9])", "different", false, 1, false, "one"),
                        serde_json::json!(0)
                    );
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn refind_churn_oversize_and_errors_do_not_change_answers() {
    use serde_json::json;
    let e = engine();
    for _ in 0..2 {
        for n in 0..32 {
            let pattern = format!("^item{n}$");
            assert_eq!(
                find(&e, &pattern, &format!("item{n}"), false, 1, false, "one"),
                json!(1)
            );
            assert_eq!(
                find(&e, &pattern, "different", false, 1, false, "one"),
                json!(0)
            );
        }
        assert_eq!(
            find(
                &e,
                &"a".repeat(300),
                &"a".repeat(300),
                false,
                1,
                false,
                "one"
            ),
            json!(1)
        );
        assert_eq!(
            find(&e, "a{10000}", &"a".repeat(10000), false, 1, false, "one"),
            json!(1)
        );
        let error = find(&e, "[", "abc", false, 1, false, "one");
        assert_eq!(
            error,
            json!({"error":"Invalid regex: regex parse error:\n    [\n    ^\nerror: unclosed character class"})
        );
        assert_eq!(find(&e, "a", "ba", false, 1, false, "one"), json!(2));
    }
}
