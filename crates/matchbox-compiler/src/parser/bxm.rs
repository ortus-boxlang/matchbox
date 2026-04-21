#[cfg(feature = "bxm")]
use regex::Regex;

#[cfg(feature = "bxm")]
pub fn transpile_bxm(source: &str) -> String {
    let mut out = String::new();

    // Tag regex for <bx:tag ...> or </bx:tag>
    let tag_re = Regex::new(r"(?is)<bx:(\w+)([^>]*?)>").unwrap();
    let end_tag_re = Regex::new(r"(?is)</bx:(\w+)>").unwrap();

    // Simple state: are we inside a bx:output?
    let mut in_output = false;

    let mut last_end = 0;

    // This is a naive implementation. For a real engine, we'd use a better parser.
    // But for a "small web server" requirement, this handles the basics.

    // Find all tags
    let mut all_tags: Vec<(usize, usize, String, bool, String)> = Vec::new(); // start, end, name, is_end, attrs

    for cap in tag_re.captures_iter(source) {
        let m = cap.get(0).unwrap();
        all_tags.push((
            m.start(),
            m.end(),
            cap[1].to_lowercase(),
            false,
            cap[2].to_string(),
        ));
    }
    for cap in end_tag_re.captures_iter(source) {
        let m = cap.get(0).unwrap();
        all_tags.push((
            m.start(),
            m.end(),
            cap[1].to_lowercase(),
            true,
            String::new(),
        ));
    }

    all_tags.sort_by_key(|t| t.0);

    for (start, end, name, is_end, attrs) in all_tags {
        // Text before this tag
        let literal = &source[last_end..start];
        if !literal.is_empty() {
            if in_output {
                out.push_str(&process_interpolation(literal));
            } else {
                out.push_str(&format!("writeOutput(\"{}\");\n", escape_string(literal)));
            }
        }

        if name == "output" {
            in_output = !is_end;
        } else if !is_end {
            // Handle other tags: bx:set, bx:if, etc.
            match name.as_str() {
                "set" => {
                    // Naive: bx:set x = 10 -> x = 10;
                    out.push_str(&format!("{};\n", attrs.trim()));
                }
                "if" => {
                    // bx:if condition="expr" -> if (expr) {
                    let condition = extract_attr(&attrs, "condition")
                        .unwrap_or_else(|| attrs.trim().to_string());
                    out.push_str(&format!("if ({}) {{\n", strip_hashes(&condition)));
                }
                "else" => {
                    out.push_str("} else {\n");
                }
                "elseif" => {
                    let condition = extract_attr(&attrs, "condition")
                        .unwrap_or_else(|| attrs.trim().to_string());
                    out.push_str(&format!("}} else if ({}) {{\n", strip_hashes(&condition)));
                }
                _ => {
                    // ignore unknown tags or treat as text
                }
            }
        } else {
            // closing tag
            match name.as_str() {
                "if" => out.push_str("}\n"),
                _ => {}
            }
        }

        last_end = end;
    }

    // Final bit of text
    let literal = &source[last_end..];
    if !literal.is_empty() {
        if in_output {
            out.push_str(&process_interpolation(literal));
        } else {
            out.push_str(&format!("writeOutput(\"{}\");\n", escape_string(literal)));
        }
    }

    out
}

#[cfg(feature = "bxm")]
fn process_interpolation(text: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    let mut in_expr = false;

    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' {
            // check for double ##
            if i + 1 < chars.len() && chars[i + 1] == '#' {
                // literal #
                let chunk = &text[last..i];
                if !chunk.is_empty() {
                    out.push_str(&format!("writeOutput(\"{}\");\n", escape_string(chunk)));
                }
                out.push_str("writeOutput(\"##\");\n");
                i += 2;
                last = i;
                continue;
            }

            let chunk = &text[last..i];
            if !chunk.is_empty() {
                out.push_str(&format!("writeOutput(\"{}\");\n", escape_string(chunk)));
            }

            if in_expr {
                // end of expr
                in_expr = false;
            } else {
                in_expr = true;
            }
            i += 1;
            last = i;
        } else {
            if in_expr {
                // find end #
                let start_expr = i;
                while i < chars.len() && chars[i] != '#' {
                    i += 1;
                }
                let expr = &text[start_expr..i];
                out.push_str(&format!("writeOutput({});\n", expr));
                // i is now at # (or end)
                last = i; // will be handled next iteration (which will see # and toggle in_expr)
            } else {
                i += 1;
            }
        }
    }

    let chunk = &text[last..];
    if !chunk.is_empty() {
        out.push_str(&format!("writeOutput(\"{}\");\n", escape_string(chunk)));
    }

    out
}

#[cfg(feature = "bxm")]
fn escape_string(s: &str) -> String {
    // MatchBox BoxLang uses "" to escape quotes and ## for literal hashes within strings
    s.replace("\"", "\"\"").replace("#", "##")
}

#[cfg(feature = "bxm")]
fn strip_hashes(s: &str) -> String {
    s.trim_matches('#').to_string()
}

#[cfg(feature = "bxm")]
fn extract_attr(attrs: &str, name: &str) -> Option<String> {
    // Naive: condition="expr"
    let re = Regex::new(&format!(r#"(?i){}\s*=\s*"([^"]*)""#, name)).unwrap();
    re.captures(attrs).map(|c| c[1].to_string())
}
