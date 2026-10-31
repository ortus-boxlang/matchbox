use crate::types::{BxNativeFunction, BxVM, BxValue};
use std::collections::HashMap;

const DEFAULT_LOCALE: &str = "__default_locale";

fn current_locale(vm: &dyn BxVM) -> String {
    vm.get_global_value(DEFAULT_LOCALE)
        .map(|value| vm.to_string(value))
        .unwrap_or_else(|| "en_US".to_string())
}

fn store_locale(vm: &mut dyn BxVM, locale: &str) {
    let value = BxValue::new_ptr(vm.string_new(locale.to_string()));
    vm.insert_global(DEFAULT_LOCALE.to_string(), value);
}

#[derive(Clone, Copy)]
struct ParsedLocale<'a> {
    language: &'a str,
    country: &'a str,
    variant: &'a str,
}

fn parse_locale(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Invalid locale: empty value".to_string());
    }

    let normalized = value.replace('-', "_");
    let alias = match normalized.to_ascii_lowercase().as_str() {
        "us"
        | "united states"
        | "english"
        | "english (us)"
        | "english (usa)"
        | "english (united states)" => Some("en_US"),
        "germany" | "german" | "german (standard)" => Some("de_DE"),
        "france" | "french" | "french (standard)" => Some("fr_FR"),
        "spain" | "spanish" | "spanish (standard)" => Some("es_ES"),
        "china" | "chinese" | "chinese (china)" => Some("zh_CN"),
        "japan" | "japanese" => Some("ja_JP"),
        "uk" | "united kingdom" | "english (uk)" | "english (united kingdom)" => Some("en_GB"),
        "turkey" | "türkiye" => Some("tr_TR"),
        _ => None,
    };
    if let Some(locale) = alias {
        return Ok(locale.to_string());
    }

    let mut parts = normalized.split('_');
    let language = parts.next().unwrap_or_default();
    let country = parts.next().unwrap_or_default();
    let variant = parts.next().unwrap_or_default();
    if !(language.len() == 2 || language.len() == 3)
        || !language.chars().all(|ch| ch.is_ascii_alphabetic())
        || (!country.is_empty()
            && (country.len() != 2 || !country.chars().all(|ch| ch.is_ascii_alphabetic())))
        || parts.next().is_some()
    {
        return Err(format!("Invalid locale: {}", value));
    }

    let mut canonical = language.to_ascii_lowercase();
    if !country.is_empty() {
        canonical.push('_');
        canonical.push_str(&country.to_ascii_uppercase());
    }
    if !variant.is_empty() {
        canonical.push('_');
        canonical.push_str(variant);
    }
    Ok(canonical)
}

fn locale_parts(locale: &str) -> ParsedLocale<'_> {
    let mut parts = locale.split('_');
    ParsedLocale {
        language: parts.next().unwrap_or_default(),
        country: parts.next().unwrap_or_default(),
        variant: parts.next().unwrap_or_default(),
    }
}

fn language_name(language: &str, display_language: &str) -> &'static str {
    match (language, display_language) {
        ("en", "de") => "Englisch",
        ("en", "zh") => "英语",
        ("en", _) => "English",
        ("de", "de") => "Deutsch",
        ("de", "zh") => "德语",
        ("de", _) => "German",
        ("fr", "de") => "Französisch",
        ("fr", "zh") => "法语",
        ("fr", _) => "French",
        ("es", "de") => "Spanisch",
        ("es", "zh") => "西班牙语",
        ("es", _) => "Spanish",
        ("ar", "zh") => "阿拉伯语",
        ("ar", _) => "Arabic",
        ("zh", "zh") => "中文",
        ("zh", _) => "Chinese",
        ("ja", "zh") => "日语",
        ("ja", _) => "Japanese",
        ("tr", "zh") => "土耳其语",
        ("tr", _) => "Turkish",
        _ => "Unknown",
    }
}

fn country_name(country: &str, display_language: &str) -> &'static str {
    match (country, display_language) {
        ("US", "de") => "Vereinigte Staaten",
        ("US", "zh") => "美国",
        ("US", _) => "United States",
        ("DE", "de") => "Deutschland",
        ("DE", "zh") => "德国",
        ("DE", _) => "Germany",
        ("FR", "de") => "Frankreich",
        ("FR", "zh") => "法国",
        ("FR", _) => "France",
        ("ES", "de") => "Spanien",
        ("ES", "zh") => "西班牙",
        ("ES", _) => "Spain",
        ("SV", "de") => "El Salvador",
        ("SV", "zh") => "萨尔瓦多",
        ("SV", _) => "El Salvador",
        ("TR", "de") => "Türkei",
        ("TR", "zh") => "土耳其",
        ("TR", _) => "Türkiye",
        ("CN", "de") => "China",
        ("CN", "zh") => "中国",
        ("CN", _) => "China",
        ("JP", "de") => "Japan",
        ("JP", "zh") => "日本",
        ("JP", _) => "Japan",
        _ => "",
    }
}

fn locale_display_name(locale: &str, display_locale: &str) -> String {
    let locale = locale_parts(locale);
    let display = locale_parts(display_locale);
    let language = language_name(locale.language, display.language);
    let country = country_name(locale.country, display.language);
    if country.is_empty() {
        language.to_string()
    } else {
        format!("{} ({})", language, country)
    }
}

fn struct_set_string(vm: &mut dyn BxVM, id: usize, key: &str, value: &str) {
    let value = BxValue::new_ptr(vm.string_new(value.to_string()));
    vm.struct_set(id, key, value);
}

pub fn register_i18n_bifs(bifs: &mut HashMap<String, BxNativeFunction>) {
    bifs.insert("getlocale".to_string(), get_locale as BxNativeFunction);
    bifs.insert("setlocale".to_string(), set_locale as BxNativeFunction);
    bifs.insert("clearlocale".to_string(), clear_locale as BxNativeFunction);
    bifs.insert(
        "currencyformat".to_string(),
        currency_format as BxNativeFunction,
    );
    bifs.insert(
        "lscurrencyformat".to_string(),
        currency_format as BxNativeFunction,
    );
    bifs.insert(
        "getlocaledisplayname".to_string(),
        get_locale_display_name as BxNativeFunction,
    );
    bifs.insert(
        "getlocaleinfo".to_string(),
        get_locale_info as BxNativeFunction,
    );
    bifs.insert("iscurrency".to_string(), is_currency as BxNativeFunction);
    bifs.insert("lsiscurrency".to_string(), is_currency as BxNativeFunction);
    bifs.insert(
        "parsecurrency".to_string(),
        parse_currency as BxNativeFunction,
    );
    bifs.insert(
        "lsparsecurrency".to_string(),
        parse_currency as BxNativeFunction,
    );
}

fn get_locale(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let locale = current_locale(vm);
    let display_name = if locale == "en_US" {
        "English (US)".to_string()
    } else {
        locale_display_name(&locale, "en_US")
    };
    Ok(BxValue::new_ptr(vm.string_new(display_name)))
}

fn set_locale(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("setLocale() expects 1 argument".to_string());
    }
    let locale = parse_locale(&vm.to_string(args[0]))?;
    store_locale(vm, &locale);
    Ok(BxValue::new_ptr(
        vm.string_new(locale_display_name(&locale, "en_US")),
    ))
}

fn clear_locale(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    store_locale(vm, "en_US");
    Ok(BxValue::new_null())
}

fn currency_format(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("currencyFormat() expects 1 argument".to_string());
    }
    let num = if args[0].is_number() {
        args[0].as_number()
    } else {
        let s = vm.to_string(args[0]);
        s.trim()
            .parse::<f64>()
            .map_err(|_| format!("currencyFormat() expected number, got '{}'", s))?
    };
    let kind = args
        .get(1)
        .map(|value| vm.to_string(*value))
        .unwrap_or_else(|| "local".to_string());
    let locale = if args.get(2).is_some_and(|value| !value.is_null()) {
        parse_locale(&vm.to_string(args[2]))?
    } else {
        current_locale(vm)
    };
    let result = format_currency(num, &kind, &locale)?;
    Ok(BxValue::new_ptr(vm.string_new(result)))
}

fn get_locale_display_name(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let locale = if args.first().is_some_and(|value| !value.is_null()) {
        parse_locale(&vm.to_string(args[0]))?
    } else {
        current_locale(vm)
    };
    let display_locale = if args.get(1).is_some_and(|value| !value.is_null()) {
        parse_locale(&vm.to_string(args[1]))?
    } else {
        locale.clone()
    };
    Ok(BxValue::new_ptr(
        vm.string_new(locale_display_name(&locale, &display_locale)),
    ))
}

fn get_locale_info(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let locale = if args.first().is_some_and(|value| !value.is_null()) {
        parse_locale(&vm.to_string(args[0]))?
    } else {
        current_locale(vm)
    };
    let display_locale = if args.get(1).is_some_and(|value| !value.is_null()) {
        parse_locale(&vm.to_string(args[1]))?
    } else {
        locale.clone()
    };
    let parsed_locale = locale_parts(&locale);
    let parsed_display_locale = locale_parts(&display_locale);
    let display_language = language_name(parsed_locale.language, parsed_display_locale.language);
    let display_country = country_name(parsed_locale.country, parsed_display_locale.language);
    let id = vm.struct_new();
    struct_set_string(vm, id, "language", parsed_locale.language);
    struct_set_string(vm, id, "country", parsed_locale.country);
    struct_set_string(vm, id, "variant", parsed_locale.variant);
    let name = locale_display_name(&locale, &display_locale);
    struct_set_string(vm, id, "name", &name);

    let display = vm.struct_new();
    struct_set_string(vm, display, "language", display_language);
    struct_set_string(vm, display, "country", display_country);
    vm.struct_set(id, "display", BxValue::new_ptr(display));

    let iso = vm.struct_new();
    struct_set_string(vm, iso, "language", iso_language(parsed_locale.language));
    struct_set_string(vm, iso, "country", iso_country(parsed_locale.country));
    vm.struct_set(id, "iso", BxValue::new_ptr(iso));
    Ok(BxValue::new_ptr(id))
}

fn iso_language(language: &str) -> &'static str {
    match language {
        "en" => "eng",
        "de" => "deu",
        "fr" => "fra",
        "es" => "spa",
        "ar" => "ara",
        "zh" => "zho",
        "ja" => "jpn",
        "tr" => "tur",
        _ => "",
    }
}

fn iso_country(country: &str) -> &'static str {
    match country {
        "US" => "USA",
        "DE" => "DEU",
        "FR" => "FRA",
        "ES" => "ESP",
        "SV" => "SLV",
        "TR" => "TUR",
        "CN" => "CHN",
        "JP" => "JPN",
        _ => "",
    }
}

fn is_currency(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("isCurrency() expects 1 argument".to_string());
    }
    let s = vm.to_string(args[0]);
    let locale = if args.get(1).is_some_and(|value| !value.is_null()) {
        parse_locale(&vm.to_string(args[1]))?
    } else {
        current_locale(vm)
    };
    let has_marker = s
        .chars()
        .any(|ch| matches!(ch, '$' | '€' | '£' | '¥' | '￥'))
        || s.contains("USD")
        || s.contains("EUR")
        || s.contains("GBP")
        || s.contains("JPY")
        || s.contains("د.أ");
    let has_known_text = s.contains("USD")
        || s.contains("EUR")
        || s.contains("GBP")
        || s.contains("JPY")
        || s.contains("CNY")
        || s.contains("JOD")
        || s.contains("د.أ");
    let has_unrecognized_text = s.chars().any(|ch| ch.is_alphabetic()) && !has_known_text;
    let valid = !has_unrecognized_text
        && (!has_marker || currency_marker_matches(&s, &locale))
        && parse_currency_value(&s, &locale).is_ok();
    Ok(BxValue::new_bool(valid))
}

fn parse_currency(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("parseCurrency() expects 1 argument".to_string());
    }
    let s = vm.to_string(args[0]);
    let locale = if args.get(1).is_some_and(|value| !value.is_null()) {
        parse_locale(&vm.to_string(args[1]))?
    } else {
        current_locale(vm)
    };
    let num = parse_currency_value(&s, &locale)
        .map_err(|_| format!("parseCurrency() cannot parse '{}'", s))?;
    Ok(BxValue::new_number(num))
}

fn format_currency(number: f64, kind: &str, locale: &str) -> Result<String, String> {
    let kind = kind.to_ascii_lowercase();
    if !matches!(kind.as_str(), "local" | "international" | "none") {
        return Err(format!("currencyFormat() has invalid type '{}'", kind));
    }
    let parts = locale_parts(locale);
    let (symbol, code, decimal, group, fraction_digits, space_before) =
        match (parts.language, parts.country) {
            ("de", _) => ('€', "EUR", ',', '.', 2, true),
            ("ja", _) => ('￥', "JPY", '.', ',', 0, false),
            ("en", "GB") => ('£', "GBP", '.', ',', 2, false),
            ("en", _) => ('$', "USD", '.', ',', 2, false),
            ("zh", _) => ('¥', "CNY", '.', ',', 2, false),
            ("ar", "JO") => ('د', "JOD", '.', ',', 3, true),
            _ => ('$', "USD", '.', ',', 2, false),
        };
    let negative = number.is_sign_negative();
    let rounded = format!("{:.*}", fraction_digits, number.abs());
    let (whole, fraction) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let grouped = group_digits(whole, group);
    let number = if fraction_digits == 0 {
        grouped
    } else {
        format!("{}{}{}", grouped, decimal, fraction)
    };
    let number = if number == "0" {
        number
    } else if negative {
        format!("-{}", number)
    } else {
        number
    };
    let result = match kind.as_str() {
        "local" if space_before => format!("{}\u{00a0}{}", symbol, number),
        "local" => format!("{}{}", symbol, number),
        "international" if space_before => format!("{} \u{00a0}{}", code, number),
        "international" => format!("{} {}", code, number),
        "none" if space_before => format!("\u{00a0}{}", number),
        "none" => number,
        _ => unreachable!(),
    };
    Ok(result)
}

fn group_digits(value: &str, separator: char) -> String {
    let mut grouped = String::new();
    for (index, ch) in value.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push(separator);
        }
        grouped.push(ch);
    }
    grouped.chars().rev().collect()
}

fn currency_marker_matches(value: &str, locale: &str) -> bool {
    let parts = locale_parts(locale);
    match parts.language {
        "ar" => value.contains("د.أ") || value.contains("JOD"),
        "de" | "fr" | "es" => value.contains('€') || value.contains("EUR"),
        "ja" => value.contains('¥') || value.contains('￥') || value.contains("JPY"),
        "zh" => value.contains('¥') || value.contains('￥') || value.contains("CNY"),
        "en" if parts.country == "GB" => value.contains('£') || value.contains("GBP"),
        "en" => value.contains('$') || value.contains("USD"),
        _ => false,
    }
}

fn parse_currency_value(value: &str, locale: &str) -> Result<f64, ()> {
    let parts = locale_parts(locale);
    let decimal_separator = if parts.language == "de" { ',' } else { '.' };
    let chars: Vec<char> = value.chars().collect();
    let mut normalized = String::new();
    let mut saw_digit = false;
    for (index, ch) in chars.iter().copied().enumerate() {
        if let Some(digit) = arabic_digit(ch) {
            normalized.push(digit);
            saw_digit = true;
            continue;
        }
        if ch == '-' && !saw_digit {
            normalized.push(ch);
            continue;
        }
        let is_decimal = (ch == decimal_separator || (parts.language == "ar" && ch == '٫'))
            && index > 0
            && index + 1 < chars.len()
            && arabic_digit(chars[index - 1]).is_some()
            && arabic_digit(chars[index + 1]).is_some();
        if is_decimal {
            normalized.push('.');
        }
    }
    if !saw_digit {
        return Err(());
    }
    normalized.parse::<f64>().map_err(|_| ())
}

fn arabic_digit(ch: char) -> Option<char> {
    match ch {
        '0'..='9' => Some(ch),
        '٠'..='٩' => char::from_u32('0' as u32 + (ch as u32 - '٠' as u32)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{get_locale, set_locale};
    use crate::{
        types::{BxVM, BxValue},
        vm::VM,
    };

    #[test]
    fn locales_are_vm_local() {
        let mut first = VM::new();
        let mut second = VM::new();
        let german = BxValue::new_ptr(first.string_new("de_DE".to_string()));

        set_locale(&mut first, &[german]).unwrap();
        let second_locale = get_locale(&mut second, &[]).unwrap();

        assert_eq!(second.to_string(second_locale), "English (US)");
    }
}
