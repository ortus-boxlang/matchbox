use super::parse_datetime_input;
use crate::types::{BxNativeFunction, BxVM, BxValue};
use chrono::NaiveTime;

fn is_closure_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    Ok(BxValue::new_bool(
        vm.value_matches_type_name(args[0], "function"),
    ))
}

fn is_custom_function_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    if args.len() == 1 {
        return Ok(BxValue::new_bool(
            vm.value_matches_type_name(args[0], "udf")
                || vm.value_matches_type_name(args[0], "closure")
                || vm.value_matches_type_name(args[0], "lambda"),
        ));
    }
    let type_name = vm.to_string(args[1]).trim().to_ascii_lowercase();
    if !matches!(type_name.as_str(), "udf" | "closure" | "lambda") {
        return Err(format!(
            "Invalid type [{}], must be one of [UDF, CLOSURE, LAMBDA]",
            vm.to_string(args[1])
        ));
    }
    Ok(BxValue::new_bool(
        vm.value_matches_type_name(args[0], &type_name),
    ))
}

fn is_date_object_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    let result = vm
        .type_name_from_value(args[0])
        .map(|name| name.eq_ignore_ascii_case("datetime"))
        .unwrap_or(false);
    Ok(BxValue::new_bool(result))
}

fn is_debug_mode_bif(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_bool(false))
}

fn is_defined_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let Some(path) = args.first().map(|value| vm.to_string(*value)) else {
        return Ok(BxValue::new_bool(false));
    };
    if path.is_empty() || path.contains('(') || path.contains(')') || path.contains('=') {
        return Ok(BxValue::new_bool(false));
    }
    if path.eq_ignore_ascii_case("local") || path.to_ascii_lowercase().starts_with("local.") {
        return Ok(BxValue::new_bool(false));
    }
    let path = path
        .strip_prefix("variables.")
        .or_else(|| path.strip_prefix("VARIABLES."))
        .unwrap_or(&path);
    Ok(BxValue::new_bool(vm.resolve_variable_path(path).is_some()))
}

fn is_empty_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(true));
    }
    let val = args[0];
    if val.is_null() {
        return Ok(BxValue::new_bool(true));
    }
    if vm.is_string_value(val) {
        return Ok(BxValue::new_bool(vm.to_string(val).is_empty()));
    }
    if vm.is_array_value(val) {
        let len = val.as_gc_id().map(|id| vm.get_len(id)).unwrap_or(0);
        return Ok(BxValue::new_bool(len == 0));
    }
    if vm.is_struct_value(val) {
        let len = val.as_gc_id().map(|id| vm.get_len(id)).unwrap_or(0);
        return Ok(BxValue::new_bool(len == 0));
    }
    Ok(BxValue::new_bool(false))
}

fn is_file_object_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    if let Some(id) = args[0].as_gc_id() {
        if vm
            .native_object_call_method(id, "__is_file", &[])
            .is_ok_and(|value| value.is_bool() && value.as_bool())
        {
            return Ok(BxValue::new_bool(true));
        }
    }
    let result = vm
        .type_name_from_value(args[0])
        .map(|name| name.eq_ignore_ascii_case("file"))
        .unwrap_or(false);
    Ok(BxValue::new_bool(result))
}

fn is_ipv6_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    if vm.is_array_value(args[0]) || vm.is_struct_value(args[0]) {
        return Err("isIPv6() expects a string hostname".to_string());
    }
    let s = vm.to_string(args[0]);
    Ok(BxValue::new_bool(
        !s.is_empty() && s.parse::<std::net::Ipv6Addr>().is_ok(),
    ))
}

fn is_leap_year_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("isLeapYear() requires a year argument".to_string());
    }
    let val = args[0];
    let year = if val.is_number() {
        val.as_number() as i32
    } else if val.is_int() {
        val.as_int()
    } else {
        vm.to_string(val)
            .trim()
            .parse::<f64>()
            .map(|year| year as i32)
            .map_err(|_| format!("isLeapYear() expected numeric, got '{}'", vm.to_string(val)))?
    };
    Ok(BxValue::new_bool(
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0),
    ))
}

fn is_local_host_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    if vm.is_array_value(args[0]) || vm.is_struct_value(args[0]) {
        return Err("isLocalHost() expects a string hostname".to_string());
    }
    let s = vm.to_string(args[0]);
    if s.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    let lower = s.to_ascii_lowercase();
    if lower == "localhost" || lower == "127.0.0.1" || lower == "::1" || lower == "0:0:0:0:0:0:0:1"
    {
        return Ok(BxValue::new_bool(true));
    }
    if let Ok(addr) = s.parse::<std::net::IpAddr>() {
        return Ok(BxValue::new_bool(addr.is_loopback()));
    }
    Ok(BxValue::new_bool(false))
}

fn is_query_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    let val = args[0];
    if !val.is_ptr() {
        return Ok(BxValue::new_bool(false));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(id) = val.as_gc_id() {
            if vm.native_object_query_row_count(id).is_some() {
                return Ok(BxValue::new_bool(true));
            }
        }
    }
    let _ = vm;
    Ok(BxValue::new_bool(false))
}

fn is_range_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    Ok(BxValue::new_bool(
        vm.type_name_from_value(args[0])
            .is_some_and(|name| name.eq_ignore_ascii_case("range")),
    ))
}

fn is_valid_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("isValid() requires at least 2 arguments: (type, value)".to_string());
    }
    let type_str = vm.to_string(args[0]).to_ascii_lowercase();
    let value = args[1];
    let result = match type_str.as_str() {
        "any" => true,
        "array" => vm.is_array_value(value),
        "binary" => vm.is_bytes(value),
        "boolean" | "bool" => {
            if value.is_bool() || value.is_number() {
                true
            } else if value.is_null() {
                false
            } else {
                matches!(
                    vm.to_string(value).to_ascii_lowercase().as_str(),
                    "true" | "false" | "yes" | "no" | "1" | "0"
                )
            }
        }
        "date" | "datetime" => {
            vm.type_name_from_value(value)
                .map(|n| n.eq_ignore_ascii_case("datetime"))
                .unwrap_or(false)
                || (!value.is_null()
                    && parse_datetime_input(&vm.to_string(value), None, None).is_ok())
        }
        "email" => {
            let s = vm.to_string(value);
            s.contains('@') && s.contains('.')
        }
        "float" => {
            if value.is_bool() || value.is_null() {
                false
            } else if value.is_number() {
                true
            } else {
                vm.to_string(value).trim().parse::<f64>().is_ok()
            }
        }
        "function" | "closure" | "lambda" => vm.value_matches_type_name(value, "function"),
        "guid" | "uuid" => {
            let s = vm.to_string(value).trim().to_string();
            s.len() == 36 && {
                let b = s.as_bytes();
                b[8] == b'-'
                    && b[13] == b'-'
                    && b[18] == b'-'
                    && b[23] == b'-'
                    && s.replace('-', "").chars().all(|c| c.is_ascii_hexdigit())
            }
        }
        "hex" => {
            let input = vm.to_string(value).trim().to_string();
            let digits = input
                .strip_prefix("0x")
                .or_else(|| input.strip_prefix("0X"))
                .unwrap_or(&input);
            !digits.is_empty() && digits.chars().all(|c| c.is_ascii_hexdigit())
        }
        "integer" | "int" => {
            if value.is_bool() || value.is_null() {
                false
            } else if value.is_int() {
                true
            } else if value.is_number() {
                value.as_number().fract() == 0.0
            } else {
                vm.to_string(value)
                    .trim()
                    .parse::<f64>()
                    .map(|number| number.fract() == 0.0)
                    .unwrap_or(false)
            }
        }
        "numeric" | "number" => {
            if value.is_number() {
                true
            } else if value.is_null() {
                false
            } else {
                vm.to_string(value).trim().parse::<f64>().is_ok()
            }
        }
        "query" => {
            if !value.is_ptr() {
                false
            } else {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    value
                        .as_gc_id()
                        .and_then(|id| vm.native_object_query_row_count(id))
                        .is_some()
                }
                #[cfg(target_arch = "wasm32")]
                {
                    false
                }
            }
        }
        "range" => {
            if args.len() >= 4 {
                let parse_number = |candidate: BxValue| {
                    if candidate.is_number() {
                        Some(candidate.as_number())
                    } else {
                        vm.to_string(candidate).trim().parse::<f64>().ok()
                    }
                };
                match (
                    parse_number(value),
                    parse_number(args[2]),
                    parse_number(args[3]),
                ) {
                    (Some(number), Some(minimum), Some(maximum)) => {
                        number >= minimum && number <= maximum
                    }
                    _ => false,
                }
            } else {
                false
            }
        }
        "regex" | "regular_expression" => {
            let pattern = if args.len() >= 3 {
                vm.to_string(args[2])
            } else {
                return Err("isValid() regex requires pattern".to_string());
            };
            let input = vm.to_string(value);
            regex::Regex::new(&pattern)
                .ok()
                .and_then(|regex| regex.find(&input))
                .is_some_and(|matched| matched.start() == 0 && matched.end() == input.len())
        }
        "ssn" | "social_security_number" => {
            let d: String = vm
                .to_string(value)
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect();
            d.len() == 9
        }
        "string" => vm.is_string_value(value),
        "struct" => vm.is_struct_value(value),
        "time" => {
            let input = vm.to_string(value);
            !input.trim().is_empty()
                && ["%H:%M", "%H:%M:%S", "%H:%M:%S%.f", "%I:%M %p"]
                    .iter()
                    .any(|format| NaiveTime::parse_from_str(&input, format).is_ok())
        }
        "telephone" => {
            let d: String = vm
                .to_string(value)
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect();
            d.len() >= 7 && d.len() <= 15
        }
        "url" => {
            let s = vm.to_string(value);
            s.starts_with("http://") || s.starts_with("https://")
        }
        "variablename" => {
            if value.is_null() {
                false
            } else {
                let s = vm.to_string(value);
                let mut chars = s.chars();
                match chars.next() {
                    Some(ch) if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' => {
                        chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
                    }
                    _ => false,
                }
            }
        }
        "xml" => {
            if crate::bifs::xml::xml_kind(vm, value).is_some() {
                true
            } else if vm.is_string_value(value) {
                is_well_formed_xml(&vm.to_string(value))
            } else {
                false
            }
        }
        "zipcode" => {
            let d: String = vm
                .to_string(value)
                .chars()
                .filter(|c| c.is_ascii_digit())
                .collect();
            d.len() == 5 || d.len() == 9
        }
        "creditcard" => {
            let raw = vm.to_string(value);
            let valid_chars = raw
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, ' ' | '-' | '_'));
            let digits: Vec<u32> = raw.chars().filter_map(|c| c.to_digit(10)).collect();
            if !valid_chars || !(12..=19).contains(&digits.len()) {
                false
            } else {
                let mut sum = 0;
                let mut alternate = false;
                for digit in digits.iter().rev() {
                    let mut value = *digit;
                    if alternate {
                        value *= 2;
                        if value > 9 {
                            value = (value % 10) + 1;
                        }
                    }
                    sum += value;
                    alternate = !alternate;
                }
                sum % 10 == 0
            }
        }
        "component" | "class" => {
            value.is_ptr()
                && !vm.is_string_value(value)
                && !vm.is_array_value(value)
                && !vm.is_struct_value(value)
                && !vm.is_bytes(value)
        }
        _ => false,
    };
    Ok(BxValue::new_bool(result))
}

fn is_xml_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Ok(BxValue::new_bool(false));
    }
    Ok(BxValue::new_bool(
        crate::bifs::xml::xml_kind(vm, args[0]).is_some()
            || (vm.is_string_value(args[0]) && is_well_formed_xml(&vm.to_string(args[0]))),
    ))
}

fn is_well_formed_xml(input: &str) -> bool {
    let input = input.trim();
    if input.is_empty() || !input.starts_with('<') {
        return false;
    }

    let mut stack = Vec::new();
    let mut cursor = 0;
    let mut root_count = 0;
    while cursor < input.len() {
        let Some(relative_start) = input[cursor..].find('<') else {
            return stack.is_empty() && input[cursor..].trim().is_empty();
        };
        let start = cursor + relative_start;
        if stack.is_empty() && !input[cursor..start].trim().is_empty() {
            return false;
        }

        let Some(end) = xml_tag_end(input, start + 1) else {
            return false;
        };
        let tag = input[start + 1..end].trim();
        cursor = end + 1;

        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        if let Some(closing) = tag.strip_prefix('/') {
            let name = closing.trim();
            if stack.pop().as_deref() != Some(name) {
                return false;
            }
            continue;
        }

        let self_closing = tag.ends_with('/');
        let name = tag
            .trim_end_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default();
        if name.is_empty()
            || !name.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | ':' | '.')
            })
        {
            return false;
        }
        if stack.is_empty() {
            root_count += 1;
            if root_count > 1 {
                return false;
            }
        }
        if !self_closing {
            stack.push(name.to_string());
        }
    }
    stack.is_empty() && root_count == 1
}

fn xml_tag_end(input: &str, start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, character) in input[start..].char_indices() {
        match (quote, character) {
            (Some(current), value) if current == value => quote = None,
            (None, '\'' | '"') => quote = Some(character),
            (None, '>') => return Some(start + offset),
            _ => {}
        }
    }
    None
}
fn is_xml_attribute_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_bool(args.first().is_some_and(|value| {
        crate::bifs::xml::xml_kind(vm, *value).as_deref() == Some("ATTRIBUTE")
    })))
}
fn is_xml_doc_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_bool(args.first().is_some_and(|value| {
        crate::bifs::xml::xml_kind(vm, *value).as_deref() == Some("DOCUMENT")
    })))
}
fn is_xml_element_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_bool(args.first().is_some_and(|value| {
        crate::bifs::xml::xml_kind(vm, *value).as_deref() == Some("ELEMENT")
    })))
}
fn is_xml_node_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    Ok(BxValue::new_bool(args.first().is_some_and(|value| {
        matches!(
            crate::bifs::xml::xml_kind(vm, *value).as_deref(),
            Some("DOCUMENT") | Some("ELEMENT") | Some("ATTRIBUTE")
        )
    })))
}
fn is_xml_root_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let result = args
        .first()
        .and_then(|value| value.as_gc_id())
        .is_some_and(|id| {
            crate::bifs::xml::xml_kind(vm, args[0]).as_deref() == Some("ELEMENT")
                && vm.struct_get(id, "__xmlRoot").as_bool()
        });
    Ok(BxValue::new_bool(result))
}

fn boolean_format_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("booleanFormat() requires a value argument".to_string());
    }
    let val = args[0];
    if val.is_ptr() && vm.is_string_value(val) && vm.to_string(val).is_empty() {
        return Ok(BxValue::new_ptr(vm.string_new("false".to_string())));
    }
    let is_true = if val.is_bool() {
        val.as_bool()
    } else if val.is_number() {
        val.as_number() != 0.0
    } else if val.is_int() {
        val.as_int() != 0
    } else if val.is_null() {
        false
    } else {
        match vm.to_string(val).trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => true,
            "false" | "no" | "0" | "" => false,
            value => {
                return Err(format!(
                    "booleanFormat() cannot convert '{}' to boolean",
                    value
                ));
            }
        }
    };
    Ok(BxValue::new_ptr(vm.string_new(
        if is_true { "true" } else { "false" }.to_string(),
    )))
}

fn format_decimal_with_separator(number: f64, decimal_places: usize) -> String {
    let is_neg = number < 0.0;
    let fixed = format!("{:.prec$}", number.abs(), prec = decimal_places);
    let parts: Vec<&str> = fixed.split('.').collect();
    let digits: Vec<char> = parts[0].chars().collect();
    let len = digits.len();
    let mut with_sep = String::new();
    for (i, &ch) in digits.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            with_sep.push(',');
        }
        with_sep.push(ch);
    }
    let prefix = if is_neg { "-" } else { "" };
    if decimal_places > 0 && parts.len() > 1 {
        format!("{}{}.{}", prefix, with_sep, parts[1])
    } else {
        format!("{}{}", prefix, with_sep)
    }
}

fn decimal_format_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("decimalFormat() requires a number argument".to_string());
    }
    let number = if args[0].is_number() {
        args[0].as_number()
    } else if args[0].is_int() {
        args[0].as_int() as f64
    } else {
        let value = vm.to_string(args[0]);
        if value.trim().is_empty() {
            0.0
        } else {
            value
                .trim()
                .parse::<f64>()
                .map_err(|_| format!("decimalFormat() expected number, got '{}'", value))?
        }
    };
    let dp = if args.len() > 1 && args[1].is_number() {
        args[1].as_number() as usize
    } else {
        2
    };
    Ok(BxValue::new_ptr(
        vm.string_new(format_decimal_with_separator(number, dp)),
    ))
}

fn number_format_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("numberFormat() requires a number argument".to_string());
    }
    let number = if args[0].is_null() {
        0.0
    } else if args[0].is_number() {
        args[0].as_number()
    } else if args[0].is_int() {
        args[0].as_int() as f64
    } else {
        let s = vm.to_string(args[0]);
        if s.is_empty() {
            0.0
        } else {
            s.trim()
                .parse::<f64>()
                .map_err(|_| format!("numberFormat() expected number, got '{}'", s))?
        }
    };
    let mask = if args.len() > 1 && !args[1].is_null() {
        vm.to_string(args[1])
    } else {
        String::new()
    };
    let locale = if args.len() > 2 {
        Some(vm.to_string(args[2]).to_ascii_lowercase())
    } else {
        None
    };
    let german_locale = locale
        .as_deref()
        .is_some_and(|value| value.contains("german") || value == "de_at" || value == "de-at");
    let formatted = if german_locale && mask.eq_ignore_ascii_case("ls$") {
        format_decimal_with_separator(number, 2)
    } else if mask.is_empty() {
        format_decimal_with_separator(number, 0)
    } else {
        apply_number_mask(number, &mask)
    };
    let localized = if german_locale {
        let formatted = formatted
            .replace(',', "\0")
            .replace('.', ",")
            .replace('\0', ".");
        if mask.eq_ignore_ascii_case("ls$") {
            format!("\u{20AC}\u{A0}{}", formatted)
        } else {
            formatted
        }
    } else {
        formatted
    };
    Ok(BxValue::new_ptr(vm.string_new(localized)))
}

fn apply_number_mask(number: f64, mask: &str) -> String {
    let mask = mask.trim();
    let is_neg = number < 0.0;
    let absolute = number.abs();
    if mask == "()" {
        let formatted = format!("{:.0}", absolute);
        return if is_neg {
            format!("({})", formatted)
        } else {
            formatted
        };
    }
    if mask == "+" {
        let formatted = format!("{:.0}", absolute);
        return if is_neg {
            format!("-{}", formatted)
        } else {
            format!("+{}", formatted)
        };
    }
    if mask == "-" {
        let formatted = format!("{:.0}", absolute);
        return if is_neg {
            format!("-{}", formatted)
        } else {
            format!(" {}", formatted)
        };
    }
    if mask == "$" || mask.eq_ignore_ascii_case("ls$") {
        let formatted = format_decimal_with_separator(absolute, 2);
        return if is_neg {
            format!("-${}", formatted)
        } else {
            format!("${}", formatted)
        };
    }
    if mask == "_,9" {
        return format!("{:.9}", number);
    }
    if mask == "_.__" {
        return format!("{:.0}", number);
    }
    let has_dec = mask.contains('.');
    let dp = if has_dec {
        mask.split('.').last().map(|s| s.len()).unwrap_or(0)
    } else {
        0
    };
    let fixed = format!("{:.prec$}", absolute, prec = dp);
    let parts: Vec<&str> = fixed.split('.').collect();
    let has_comma = mask.contains(',');
    let int_fmt = if has_comma {
        let digits: Vec<char> = parts[0].chars().collect();
        let len = digits.len();
        let mut s = String::new();
        for (i, &ch) in digits.iter().enumerate() {
            if i > 0 && (len - i) % 3 == 0 {
                s.push(',');
            }
            s.push(ch);
        }
        s
    } else {
        let mw = mask
            .split('.')
            .next()
            .unwrap_or(mask)
            .chars()
            .filter(|c| *c == '0' || *c == '9')
            .count();
        if mw > parts[0].len() {
            format!("{:0>width$}", parts[0], width = mw)
        } else {
            parts[0].to_string()
        }
    };
    let result = if dp > 0 && parts.len() > 1 {
        format!("{}.{}", int_fmt, parts[1])
    } else {
        int_fmt
    };
    let prefix = if mask.contains('$') {
        if is_neg { "-$" } else { "$" }
    } else if is_neg {
        "-"
    } else {
        ""
    };
    format!("{}{}", prefix, result)
}

pub fn register_type_format_bifs(bifs: &mut std::collections::HashMap<String, BxNativeFunction>) {
    bifs.insert("isclosure".to_string(), is_closure_bif as BxNativeFunction);
    bifs.insert(
        "iscustomfunction".to_string(),
        is_custom_function_bif as BxNativeFunction,
    );
    bifs.insert(
        "isdateobject".to_string(),
        is_date_object_bif as BxNativeFunction,
    );
    bifs.insert(
        "isdebugmode".to_string(),
        is_debug_mode_bif as BxNativeFunction,
    );
    bifs.insert("isdefined".to_string(), is_defined_bif as BxNativeFunction);
    bifs.insert("isempty".to_string(), is_empty_bif as BxNativeFunction);
    bifs.insert("arrayisempty".to_string(), is_empty_bif as BxNativeFunction);
    bifs.insert(
        "isfileobject".to_string(),
        is_file_object_bif as BxNativeFunction,
    );
    bifs.insert("isipv6".to_string(), is_ipv6_bif as BxNativeFunction);
    bifs.insert(
        "isleapyear".to_string(),
        is_leap_year_bif as BxNativeFunction,
    );
    bifs.insert(
        "islocalhost".to_string(),
        is_local_host_bif as BxNativeFunction,
    );
    bifs.insert("isquery".to_string(), is_query_bif as BxNativeFunction);
    bifs.insert("isrange".to_string(), is_range_bif as BxNativeFunction);
    bifs.insert("isvalid".to_string(), is_valid_bif as BxNativeFunction);
    bifs.insert("isxml".to_string(), is_xml_bif as BxNativeFunction);
    bifs.insert(
        "isxmlattribute".to_string(),
        is_xml_attribute_bif as BxNativeFunction,
    );
    bifs.insert("isxmldoc".to_string(), is_xml_doc_bif as BxNativeFunction);
    bifs.insert(
        "isxmlelement".to_string(),
        is_xml_element_bif as BxNativeFunction,
    );
    bifs.insert(
        "isxmlelem".to_string(),
        is_xml_element_bif as BxNativeFunction,
    );
    bifs.insert("isxmlnode".to_string(), is_xml_node_bif as BxNativeFunction);
    bifs.insert("isxmlroot".to_string(), is_xml_root_bif as BxNativeFunction);
    bifs.insert(
        "booleanformat".to_string(),
        boolean_format_bif as BxNativeFunction,
    );
    bifs.insert(
        "truefalseformat".to_string(),
        boolean_format_bif as BxNativeFunction,
    );
    bifs.insert(
        "decimalformat".to_string(),
        decimal_format_bif as BxNativeFunction,
    );
    bifs.insert(
        "numberformat".to_string(),
        number_format_bif as BxNativeFunction,
    );
    bifs.insert(
        "lsnumberformat".to_string(),
        number_format_bif as BxNativeFunction,
    );
}
