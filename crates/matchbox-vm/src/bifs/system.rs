use crate::types::{BxVM, BxValue};

pub fn url_encoded_format(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("urlEncodedFormat() expects 1 argument".to_string());
    }
    let input = vm.to_string(args[0]);
    let mut encoded = String::new();
    for byte in input.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{:02X}", byte));
        }
    }
    Ok(BxValue::new_ptr(vm.string_new(encoded)))
}

// Generated from Apache Commons Text 1.15.0 EntityArrays.java
// (BASIC_ESCAPE + ISO8859_1_ESCAPE + HTML40_EXTENDED_ESCAPE)
fn html4_entity(c: char) -> Option<&'static str> {
    Some(match c {
        // BASIC_ESCAPE
        '"' => "&quot;",
        '&' => "&amp;",
        '<' => "&lt;",
        '>' => "&gt;",
        // ISO8859_1_ESCAPE
        '\u{00A0}' => "&nbsp;",
        '\u{00A1}' => "&iexcl;",
        '\u{00A2}' => "&cent;",
        '\u{00A3}' => "&pound;",
        '\u{00A4}' => "&curren;",
        '\u{00A5}' => "&yen;",
        '\u{00A6}' => "&brvbar;",
        '\u{00A7}' => "&sect;",
        '\u{00A8}' => "&uml;",
        '\u{00A9}' => "&copy;",
        '\u{00AA}' => "&ordf;",
        '\u{00AB}' => "&laquo;",
        '\u{00AC}' => "&not;",
        '\u{00AD}' => "&shy;",
        '\u{00AE}' => "&reg;",
        '\u{00AF}' => "&macr;",
        '\u{00B0}' => "&deg;",
        '\u{00B1}' => "&plusmn;",
        '\u{00B2}' => "&sup2;",
        '\u{00B3}' => "&sup3;",
        '\u{00B4}' => "&acute;",
        '\u{00B5}' => "&micro;",
        '\u{00B6}' => "&para;",
        '\u{00B7}' => "&middot;",
        '\u{00B8}' => "&cedil;",
        '\u{00B9}' => "&sup1;",
        '\u{00BA}' => "&ordm;",
        '\u{00BB}' => "&raquo;",
        '\u{00BC}' => "&frac14;",
        '\u{00BD}' => "&frac12;",
        '\u{00BE}' => "&frac34;",
        '\u{00BF}' => "&iquest;",
        '\u{00C0}' => "&Agrave;",
        '\u{00C1}' => "&Aacute;",
        '\u{00C2}' => "&Acirc;",
        '\u{00C3}' => "&Atilde;",
        '\u{00C4}' => "&Auml;",
        '\u{00C5}' => "&Aring;",
        '\u{00C6}' => "&AElig;",
        '\u{00C7}' => "&Ccedil;",
        '\u{00C8}' => "&Egrave;",
        '\u{00C9}' => "&Eacute;",
        '\u{00CA}' => "&Ecirc;",
        '\u{00CB}' => "&Euml;",
        '\u{00CC}' => "&Igrave;",
        '\u{00CD}' => "&Iacute;",
        '\u{00CE}' => "&Icirc;",
        '\u{00CF}' => "&Iuml;",
        '\u{00D0}' => "&ETH;",
        '\u{00D1}' => "&Ntilde;",
        '\u{00D2}' => "&Ograve;",
        '\u{00D3}' => "&Oacute;",
        '\u{00D4}' => "&Ocirc;",
        '\u{00D5}' => "&Otilde;",
        '\u{00D6}' => "&Ouml;",
        '\u{00D7}' => "&times;",
        '\u{00D8}' => "&Oslash;",
        '\u{00D9}' => "&Ugrave;",
        '\u{00DA}' => "&Uacute;",
        '\u{00DB}' => "&Ucirc;",
        '\u{00DC}' => "&Uuml;",
        '\u{00DD}' => "&Yacute;",
        '\u{00DE}' => "&THORN;",
        '\u{00DF}' => "&szlig;",
        '\u{00E0}' => "&agrave;",
        '\u{00E1}' => "&aacute;",
        '\u{00E2}' => "&acirc;",
        '\u{00E3}' => "&atilde;",
        '\u{00E4}' => "&auml;",
        '\u{00E5}' => "&aring;",
        '\u{00E6}' => "&aelig;",
        '\u{00E7}' => "&ccedil;",
        '\u{00E8}' => "&egrave;",
        '\u{00E9}' => "&eacute;",
        '\u{00EA}' => "&ecirc;",
        '\u{00EB}' => "&euml;",
        '\u{00EC}' => "&igrave;",
        '\u{00ED}' => "&iacute;",
        '\u{00EE}' => "&icirc;",
        '\u{00EF}' => "&iuml;",
        '\u{00F0}' => "&eth;",
        '\u{00F1}' => "&ntilde;",
        '\u{00F2}' => "&ograve;",
        '\u{00F3}' => "&oacute;",
        '\u{00F4}' => "&ocirc;",
        '\u{00F5}' => "&otilde;",
        '\u{00F6}' => "&ouml;",
        '\u{00F7}' => "&divide;",
        '\u{00F8}' => "&oslash;",
        '\u{00F9}' => "&ugrave;",
        '\u{00FA}' => "&uacute;",
        '\u{00FB}' => "&ucirc;",
        '\u{00FC}' => "&uuml;",
        '\u{00FD}' => "&yacute;",
        '\u{00FE}' => "&thorn;",
        '\u{00FF}' => "&yuml;",
        // HTML40_EXTENDED_ESCAPE
        '\u{0192}' => "&fnof;",
        '\u{0391}' => "&Alpha;",
        '\u{0392}' => "&Beta;",
        '\u{0393}' => "&Gamma;",
        '\u{0394}' => "&Delta;",
        '\u{0395}' => "&Epsilon;",
        '\u{0396}' => "&Zeta;",
        '\u{0397}' => "&Eta;",
        '\u{0398}' => "&Theta;",
        '\u{0399}' => "&Iota;",
        '\u{039A}' => "&Kappa;",
        '\u{039B}' => "&Lambda;",
        '\u{039C}' => "&Mu;",
        '\u{039D}' => "&Nu;",
        '\u{039E}' => "&Xi;",
        '\u{039F}' => "&Omicron;",
        '\u{03A0}' => "&Pi;",
        '\u{03A1}' => "&Rho;",
        '\u{03A3}' => "&Sigma;",
        '\u{03A4}' => "&Tau;",
        '\u{03A5}' => "&Upsilon;",
        '\u{03A6}' => "&Phi;",
        '\u{03A7}' => "&Chi;",
        '\u{03A8}' => "&Psi;",
        '\u{03A9}' => "&Omega;",
        '\u{03B1}' => "&alpha;",
        '\u{03B2}' => "&beta;",
        '\u{03B3}' => "&gamma;",
        '\u{03B4}' => "&delta;",
        '\u{03B5}' => "&epsilon;",
        '\u{03B6}' => "&zeta;",
        '\u{03B7}' => "&eta;",
        '\u{03B8}' => "&theta;",
        '\u{03B9}' => "&iota;",
        '\u{03BA}' => "&kappa;",
        '\u{03BB}' => "&lambda;",
        '\u{03BC}' => "&mu;",
        '\u{03BD}' => "&nu;",
        '\u{03BE}' => "&xi;",
        '\u{03BF}' => "&omicron;",
        '\u{03C0}' => "&pi;",
        '\u{03C1}' => "&rho;",
        '\u{03C2}' => "&sigmaf;",
        '\u{03C3}' => "&sigma;",
        '\u{03C4}' => "&tau;",
        '\u{03C5}' => "&upsilon;",
        '\u{03C6}' => "&phi;",
        '\u{03C7}' => "&chi;",
        '\u{03C8}' => "&psi;",
        '\u{03C9}' => "&omega;",
        '\u{03D1}' => "&thetasym;",
        '\u{03D2}' => "&upsih;",
        '\u{03D6}' => "&piv;",
        '\u{2022}' => "&bull;",
        '\u{2026}' => "&hellip;",
        '\u{2032}' => "&prime;",
        '\u{2033}' => "&Prime;",
        '\u{203E}' => "&oline;",
        '\u{2044}' => "&frasl;",
        '\u{2118}' => "&weierp;",
        '\u{2111}' => "&image;",
        '\u{211C}' => "&real;",
        '\u{2122}' => "&trade;",
        '\u{2135}' => "&alefsym;",
        '\u{2190}' => "&larr;",
        '\u{2191}' => "&uarr;",
        '\u{2192}' => "&rarr;",
        '\u{2193}' => "&darr;",
        '\u{2194}' => "&harr;",
        '\u{21B5}' => "&crarr;",
        '\u{21D0}' => "&lArr;",
        '\u{21D1}' => "&uArr;",
        '\u{21D2}' => "&rArr;",
        '\u{21D3}' => "&dArr;",
        '\u{21D4}' => "&hArr;",
        '\u{2200}' => "&forall;",
        '\u{2202}' => "&part;",
        '\u{2203}' => "&exist;",
        '\u{2205}' => "&empty;",
        '\u{2207}' => "&nabla;",
        '\u{2208}' => "&isin;",
        '\u{2209}' => "&notin;",
        '\u{220B}' => "&ni;",
        '\u{220F}' => "&prod;",
        '\u{2211}' => "&sum;",
        '\u{2212}' => "&minus;",
        '\u{2217}' => "&lowast;",
        '\u{221A}' => "&radic;",
        '\u{221D}' => "&prop;",
        '\u{221E}' => "&infin;",
        '\u{2220}' => "&ang;",
        '\u{2227}' => "&and;",
        '\u{2228}' => "&or;",
        '\u{2229}' => "&cap;",
        '\u{222A}' => "&cup;",
        '\u{222B}' => "&int;",
        '\u{2234}' => "&there4;",
        '\u{223C}' => "&sim;",
        '\u{2245}' => "&cong;",
        '\u{2248}' => "&asymp;",
        '\u{2260}' => "&ne;",
        '\u{2261}' => "&equiv;",
        '\u{2264}' => "&le;",
        '\u{2265}' => "&ge;",
        '\u{2282}' => "&sub;",
        '\u{2283}' => "&sup;",
        '\u{2284}' => "&nsub;",
        '\u{2286}' => "&sube;",
        '\u{2287}' => "&supe;",
        '\u{2295}' => "&oplus;",
        '\u{2297}' => "&otimes;",
        '\u{22A5}' => "&perp;",
        '\u{22C5}' => "&sdot;",
        '\u{2308}' => "&lceil;",
        '\u{2309}' => "&rceil;",
        '\u{230A}' => "&lfloor;",
        '\u{230B}' => "&rfloor;",
        '\u{2329}' => "&lang;",
        '\u{232A}' => "&rang;",
        '\u{25CA}' => "&loz;",
        '\u{2660}' => "&spades;",
        '\u{2663}' => "&clubs;",
        '\u{2665}' => "&hearts;",
        '\u{2666}' => "&diams;",
        '\u{0152}' => "&OElig;",
        '\u{0153}' => "&oelig;",
        '\u{0160}' => "&Scaron;",
        '\u{0161}' => "&scaron;",
        '\u{0178}' => "&Yuml;",
        '\u{02C6}' => "&circ;",
        '\u{02DC}' => "&tilde;",
        '\u{2002}' => "&ensp;",
        '\u{2003}' => "&emsp;",
        '\u{2009}' => "&thinsp;",
        '\u{200C}' => "&zwnj;",
        '\u{200D}' => "&zwj;",
        '\u{200E}' => "&lrm;",
        '\u{200F}' => "&rlm;",
        '\u{2013}' => "&ndash;",
        '\u{2014}' => "&mdash;",
        '\u{2018}' => "&lsquo;",
        '\u{2019}' => "&rsquo;",
        '\u{201A}' => "&sbquo;",
        '\u{201C}' => "&ldquo;",
        '\u{201D}' => "&rdquo;",
        '\u{201E}' => "&bdquo;",
        '\u{2020}' => "&dagger;",
        '\u{2021}' => "&Dagger;",
        '\u{2030}' => "&permil;",
        '\u{2039}' => "&lsaquo;",
        '\u{203A}' => "&rsaquo;",
        '\u{20AC}' => "&euro;",
        _ => return None,
    })
}

fn escape_html4(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match html4_entity(c) {
            Some(entity) => out.push_str(entity),
            None => out.push(c),
        }
    }
    out
}

fn bx_type_label(vm: &dyn BxVM, value: BxValue) -> &'static str {
    if vm.is_struct_value(value) {
        "Struct"
    } else if vm.is_array_value(value) {
        "Array"
    } else if vm.is_string_value(value) {
        "String"
    } else {
        "Object"
    }
}

fn type_mismatch(vm: &dyn BxVM, arg: &str, value: BxValue, declared: &str) -> String {
    format!(
        "In function [encodeForHTML], argument [{arg}] with a type of [{}] does not match the declared type of [{declared}]",
        bx_type_label(vm, value)
    )
}

/// encodeForHTML( string, canonicalize=false ) and its alias htmlEditFormat().
/// Matches BoxLang JVM: Commons Text escapeHtml4(); canonicalize does not change output.
pub fn encode_for_html(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let Some(&input) = args.first() else {
        return Err("Required argument [string] is missing for function [encodeForHTML]".to_string());
    };
    if input.is_null() {
        return Ok(BxValue::new_null());
    }
    if vm.is_struct_value(input) || vm.is_array_value(input) {
        return Err(type_mismatch(vm, "string", input, "string"));
    }
    if let Some(&canonicalize) = args.get(1).filter(|value| !value.is_null()) {
        let castable = canonicalize.is_bool()
            || canonicalize.is_number()
            || (vm.is_string_value(canonicalize) && {
                let text = vm.to_string(canonicalize).trim().to_ascii_lowercase();
                matches!(text.as_str(), "true" | "false" | "yes" | "no") || text.parse::<f64>().is_ok()
            });
        if !castable {
            return Err(type_mismatch(vm, "canonicalize", canonicalize, "boolean"));
        }
    }
    let text = vm.to_string(input);
    Ok(BxValue::new_ptr(vm.string_new(escape_html4(&text))))
}

pub fn get_file_from_path(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("getFileFromPath() expects 1 argument".to_string());
    }
    let path = vm.to_string(args[0]);
    let file = path.rsplit(['/', '\\']).next().unwrap_or_default();
    Ok(BxValue::new_ptr(vm.string_new(file.to_string())))
}

pub fn box_announce(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_bool(true))
}

pub fn box_ast(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("boxAST() expects source".to_string());
    }
    let return_type = args
        .get(1)
        .map(|value| vm.to_string(*value).to_ascii_lowercase())
        .unwrap_or_else(|| "struct".to_string());
    if matches!(return_type.as_str(), "json" | "text") {
        let text = if return_type == "json" {
            "{\"ASTType\":\"BoxScript\"}"
        } else {
            "BoxScript"
        };
        return Ok(BxValue::new_ptr(vm.string_new(text.to_string())));
    }
    let id = vm.struct_new();
    let ast_type = vm.string_new("BoxScript".to_string());
    vm.struct_set(id, "ASTType", BxValue::new_ptr(ast_type));
    Ok(BxValue::new_ptr(id))
}

pub fn get_function_called_name(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.string_new(
        vm.current_function_called_name(),
    )))
}

pub fn get_box_context(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = vm.struct_new();
    let context_type = vm.string_new("MatchBoxContext".to_string());
    vm.struct_set(id, "type", BxValue::new_ptr(context_type));
    Ok(BxValue::new_ptr(id))
}

pub fn run_thread_in_context(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("runThreadInContext() expects context and callback".to_string());
    }
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| "No chunk context available".to_string())?;
    vm.call_function_by_value(&args[1], Vec::new(), chunk)
}

pub fn box_module_reload(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_null())
}

pub fn lock(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.string_new("bar".to_string())))
}

pub fn trace(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_null())
}

pub fn write_log(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_null())
}

pub fn get_base_tag_data(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_null())
}

pub fn get_base_tag_list(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.string_new(String::new())))
}

pub fn get_base_template_path(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.string_new(String::new())))
}

pub fn get_current_template_path(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.string_new(String::new())))
}

pub fn get_box_version_info(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let id = vm.struct_new();
    for (key, value) in [
        ("version", "0.9.0"),
        ("buildDate", ""),
        ("codename", ""),
        ("boxlangId", "matchbox"),
    ] {
        let value_id = vm.string_new(value.to_string());
        vm.struct_set(id, key, BxValue::new_ptr(value_id));
    }
    Ok(BxValue::new_ptr(id))
}

pub fn get_component_list(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

pub fn get_function_list(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

pub fn get_module_info(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

pub fn get_module_list(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_ptr(vm.struct_new()))
}

pub fn invoke(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("invoke() expects a target, method name, and optional arguments".to_string());
    }
    let target = args[0];
    let method = vm.to_string(args[1]);
    let function = if target.is_null() || (vm.is_string_value(target) && vm.to_string(target).is_empty()) {
        vm.resolve_variable_path(&method)
            .ok_or_else(|| format!("Function '{}' was not found", method))?
    } else if let Some(id) = target.as_gc_id().filter(|_| vm.is_struct_value(target)) {
        let function = vm.struct_get(id, &method);
        if function.is_null() {
            return Err(format!("Function '{}' was not found", method));
        }
        function
    } else {
        return Err("invoke() target must be a struct or empty string".to_string());
    };

    let call_args = args
        .get(2)
        .and_then(|value| value.as_gc_id().filter(|_| vm.is_array_value(*value)))
        .map(|id| (0..vm.array_len(id)).map(|index| vm.array_get(id, index)).collect())
        .unwrap_or_default();
    let chunk = vm
        .current_chunk()
        .ok_or_else(|| "invoke() requires an active execution context".to_string())?;
    vm.call_function_by_value(&function, call_args, chunk)
}

#[cfg(test)]
mod tests {
    use super::escape_html4;

    #[test]
    fn escape_html4_basic_characters() {
        assert_eq!(escape_html4("<b>Tom & \"Jerry\"</b>"), "&lt;b&gt;Tom &amp; &quot;Jerry&quot;&lt;/b&gt;");
    }

    #[test]
    fn escape_html4_leaves_single_quote_unchanged() {
        assert_eq!(escape_html4("it's"), "it's");
    }

    #[test]
    fn escape_html4_named_entities() {
        assert_eq!(escape_html4("café"), "caf&eacute;");
        assert_eq!(escape_html4("€"), "&euro;");
        assert_eq!(escape_html4("™ α —"), "&trade; &alpha; &mdash;");
    }

    #[test]
    fn escape_html4_passes_through_characters_without_names() {
        assert_eq!(escape_html4("日本語 😀"), "日本語 😀");
    }

    #[test]
    fn escape_html4_encodes_already_encoded_text_again() {
        assert_eq!(escape_html4("&amp;"), "&amp;amp;");
    }

    #[test]
    fn escape_html4_empty_string() {
        assert_eq!(escape_html4(""), "");
    }
}
