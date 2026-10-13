use crate::types::{BxNativeFunction, BxVM, BxValue};
use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Timelike, Utc};
use std::collections::HashMap;
use std::iter::Peekable;
use std::str::Chars;

use super::{datetime_from_parts, parse_datetime_input, parse_timezone_offset};

pub fn register_math_datetime_bifs(bifs: &mut HashMap<String, BxNativeFunction>) {
    bifs.insert(
        "incrementvalue".to_string(),
        increment_value as BxNativeFunction,
    );
    bifs.insert(
        "decrementvalue".to_string(),
        decrement_value as BxNativeFunction,
    );
    bifs.insert("fix".to_string(), fix_bif as BxNativeFunction);
    bifs.insert("formatbasen".to_string(), format_base_n as BxNativeFunction);
    bifs.insert("inputbasen".to_string(), input_base_n as BxNativeFunction);
    bifs.insert("sgn".to_string(), sgn_bif as BxNativeFunction);
    bifs.insert("sqr".to_string(), sqr_bif as BxNativeFunction);
    bifs.insert(
        "precisionevaluate".to_string(),
        precision_evaluate as BxNativeFunction,
    );
    bifs.insert("createtime".to_string(), create_time as BxNativeFunction);
    bifs.insert(
        "createtimespan".to_string(),
        create_time_span as BxNativeFunction,
    );
    bifs.insert("datecompare".to_string(), date_compare as BxNativeFunction);
    bifs.insert("dateconvert".to_string(), date_convert as BxNativeFunction);
    bifs.insert("datepart".to_string(), date_part as BxNativeFunction);
    bifs.insert(
        "gettimezoneinfo".to_string(),
        get_timezone_info as BxNativeFunction,
    );
    bifs.insert("settimezone".to_string(), set_timezone as BxNativeFunction);
    bifs.insert(
        "cleartimezone".to_string(),
        clear_timezone as BxNativeFunction,
    );
    bifs.insert(
        "createodbcdatetime".to_string(),
        create_odbc_date_time as BxNativeFunction,
    );
    bifs.insert(
        "createodbcdate".to_string(),
        create_odbc_date as BxNativeFunction,
    );
    bifs.insert(
        "createodbctime".to_string(),
        create_odbc_time as BxNativeFunction,
    );
    bifs.insert("timeunits".to_string(), time_units as BxNativeFunction);
    bifs.insert("year".to_string(), year_bif as BxNativeFunction);
    bifs.insert("quarter".to_string(), quarter_bif as BxNativeFunction);
    bifs.insert("month".to_string(), month_bif as BxNativeFunction);
    bifs.insert(
        "monthasstring".to_string(),
        month_as_string_bif as BxNativeFunction,
    );
    bifs.insert(
        "monthshortasstring".to_string(),
        month_short_as_string_bif as BxNativeFunction,
    );
    bifs.insert("day".to_string(), day_bif as BxNativeFunction);
    bifs.insert(
        "daysinmonth".to_string(),
        days_in_month_bif as BxNativeFunction,
    );
    bifs.insert(
        "daysinyear".to_string(),
        days_in_year_bif as BxNativeFunction,
    );
    bifs.insert("dayofweek".to_string(), day_of_week_bif as BxNativeFunction);
    bifs.insert(
        "dayofweekasstring".to_string(),
        day_of_week_as_string_bif as BxNativeFunction,
    );
    bifs.insert(
        "dayofweekshortasstring".to_string(),
        day_of_week_short_as_string_bif as BxNativeFunction,
    );
    bifs.insert("dayofyear".to_string(), day_of_year_bif as BxNativeFunction);
    bifs.insert(
        "firstdayofmonth".to_string(),
        first_day_of_month_bif as BxNativeFunction,
    );
    bifs.insert("week".to_string(), week_bif as BxNativeFunction);
    bifs.insert("hour".to_string(), hour_bif as BxNativeFunction);
    bifs.insert("minute".to_string(), minute_bif as BxNativeFunction);
    bifs.insert("second".to_string(), second_bif as BxNativeFunction);
    bifs.insert(
        "millisecond".to_string(),
        millisecond_bif as BxNativeFunction,
    );
    bifs.insert("nanosecond".to_string(), nanosecond_bif as BxNativeFunction);
}

// ============================================================================
// Math BIFs
// ============================================================================

fn increment_value(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("incrementValue() expects 1 argument".to_string());
    }
    Ok(BxValue::new_number(
        ((args[0].as_number() + 1.0) * 1e12).round() / 1e12,
    ))
}

fn decrement_value(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("decrementValue() expects 1 argument".to_string());
    }
    Ok(BxValue::new_number(
        ((args[0].as_number() - 1.0) * 1e12).round() / 1e12,
    ))
}

fn fix_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fix() expects 1 argument".to_string());
    }
    let n = args[0].as_number();
    let result = if n > 0.0 {
        n.floor()
    } else if n < 0.0 {
        n.ceil()
    } else {
        0.0
    };
    Ok(BxValue::new_number(result))
}

fn format_base_n(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("formatBaseN() expects 2 arguments: (number, radix)".to_string());
    }
    let number = args[0].as_number() as i64;
    let radix = args[1].as_number() as u32;
    if radix < 2 || radix > 36 {
        return Err("Radix must be between 2 and 36".to_string());
    }
    let result = format_radix(number, radix);
    let ptr = vm.string_new(result);
    Ok(BxValue::new_ptr(ptr))
}

fn format_radix(number: i64, radix: u32) -> String {
    if number == 0 {
        return "0".to_string();
    }
    let negative = number < 0;
    let digits = "0123456789abcdefghijklmnopqrstuvwxyz";
    let mut result = Vec::new();
    let r = radix as u64;
    let mut n = number.unsigned_abs();
    while n > 0 {
        let rem = (n % r) as usize;
        result.push(digits.as_bytes()[rem] as char);
        n /= r;
    }
    if negative {
        result.push('-');
    }
    result.into_iter().rev().collect()
}

fn input_base_n(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("inputBaseN() expects 2 arguments: (string, radix)".to_string());
    }
    let raw = vm.to_string(args[0]).trim().to_string();
    let radix = args[1].as_number() as u32;
    if radix < 2 || radix > 36 {
        return Err("Radix must be between 2 and 36".to_string());
    }
    let s = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or(&raw);
    let result =
        i64::from_str_radix(s, radix).map_err(|e| format!("inputBaseN() invalid input: {}", e))?;
    Ok(BxValue::new_number(result as f64))
}

fn sgn_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("sgn() expects 1 argument".to_string());
    }
    let n = args[0].as_number();
    let result = if n > 0.0 {
        1.0
    } else if n < 0.0 {
        -1.0
    } else {
        0.0
    };
    Ok(BxValue::new_number(result))
}

fn sqr_bif(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("sqr() expects 1 argument".to_string());
    }
    let n = args[0].as_number();
    if n < 0.0 {
        return Err("sqr() cannot calculate the square root of a negative number".to_string());
    }
    Ok(BxValue::new_number(n.sqrt()))
}

fn precision_evaluate(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("precisionEvaluate() expects 1 argument".to_string());
    }
    let expr = vm.to_string(args[0]);
    let mut parser = PrecisionParser::new(&expr);
    let value = parser.parse_expression()?;
    parser.skip_whitespace();
    if parser.input.peek().is_some() {
        return Err("precisionEvaluate() could not parse the expression".to_string());
    }
    Ok(BxValue::new_number(value))
}

struct PrecisionParser<'a> {
    input: Peekable<Chars<'a>>,
}

impl<'a> PrecisionParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.chars().peekable(),
        }
    }

    fn skip_whitespace(&mut self) {
        while self.input.peek().is_some_and(|ch| ch.is_whitespace()) {
            self.input.next();
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        if self.input.peek() == Some(&expected) {
            self.input.next();
            true
        } else {
            false
        }
    }

    fn parse_expression(&mut self) -> Result<f64, String> {
        let mut value = self.parse_term()?;
        loop {
            if self.consume('+') {
                value += self.parse_term()?;
            } else if self.consume('-') {
                value -= self.parse_term()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_term(&mut self) -> Result<f64, String> {
        let mut value = self.parse_power()?;
        loop {
            if self.consume('*') {
                value *= self.parse_power()?;
            } else if self.consume('/') {
                value /= self.parse_power()?;
            } else if self.consume_word("mod") {
                value %= self.parse_power()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_power(&mut self) -> Result<f64, String> {
        let value = self.parse_unary()?;
        if self.consume('^') {
            Ok(value.powf(self.parse_power()?))
        } else {
            Ok(value)
        }
    }

    fn parse_unary(&mut self) -> Result<f64, String> {
        if self.consume('-') {
            Ok(-self.parse_unary()?)
        } else if self.consume('+') {
            self.parse_unary()
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<f64, String> {
        if self.consume('(') {
            let value = self.parse_expression()?;
            if !self.consume(')') {
                return Err("precisionEvaluate() expected ')'".to_string());
            }
            return Ok(value);
        }
        self.skip_whitespace();
        let mut number = String::new();
        while self
            .input
            .peek()
            .is_some_and(|ch| ch.is_ascii_digit() || *ch == '.')
        {
            number.push(self.input.next().unwrap());
        }
        number
            .parse::<f64>()
            .map_err(|_| "precisionEvaluate() expected a number".to_string())
    }

    fn consume_word(&mut self, word: &str) -> bool {
        self.skip_whitespace();
        let mut chars = self.input.clone();
        for expected in word.chars() {
            if chars
                .next()
                .is_none_or(|actual| !actual.eq_ignore_ascii_case(&expected))
            {
                return false;
            }
        }
        if chars.peek().is_some_and(|ch| ch.is_ascii_alphabetic()) {
            return false;
        }
        for _ in word.chars() {
            self.input.next();
        }
        true
    }
}

// ============================================================================
// Date/Time BIFs
// ============================================================================

fn create_time(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let hour = args.first().map(|v| v.as_number() as u32).unwrap_or(0);
    let minute = args.get(1).map(|v| v.as_number() as u32).unwrap_or(0);
    let second = args.get(2).map(|v| v.as_number() as u32).unwrap_or(0);
    let millis = args.get(3).map(|v| v.as_number() as u32).unwrap_or(0);
    let tz = args.get(4).map(|v| vm.to_string(*v));
    let dt = datetime_from_parts(1970, 1, 1, hour, minute, second, millis, tz.as_deref())?;
    Ok(BxValue::new_ptr(vm.datetime_new(dt)))
}

fn create_time_span(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 4 {
        return Err(
            "createTimeSpan() expects at least 4 arguments: (days, hours, minutes, seconds)"
                .to_string(),
        );
    }
    let days = args[0].as_number();
    let hours = args[1].as_number();
    let minutes = args[2].as_number();
    let seconds = args[3].as_number();
    let millis = args.get(4).map(|v| v.as_number()).unwrap_or(0.0);
    let total_millis =
        days * 86_400_000.0 + hours * 3_600_000.0 + minutes * 60_000.0 + seconds * 1_000.0 + millis;
    let total_seconds = total_millis / 1_000.0;
    Ok(BxValue::new_number(total_seconds))
}

fn date_compare(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("dateCompare() expects at least 2 arguments".to_string());
    }
    let datepart = args.get(2).map(|v| vm.to_string(*v));
    let dt1 = parse_datetime_input(&vm.to_string(args[0]), None, None)?;
    let dt2 = parse_datetime_input(&vm.to_string(args[1]), None, None)?;

    let result = match datepart
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("s" | "second" | "seconds") => {
            let diff = dt1.timestamp() - dt2.timestamp();
            if diff == 0 {
                0
            } else if diff < 0 {
                -1
            } else {
                1
            }
        }
        Some("n" | "minute" | "minutes") => {
            let d1 = dt1.with_second(0).unwrap().with_nanosecond(0).unwrap();
            let d2 = dt2.with_second(0).unwrap().with_nanosecond(0).unwrap();
            match d1.cmp(&d2) {
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Greater => 1,
            }
        }
        Some("h" | "hour" | "hours") => {
            let d1 = dt1
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap();
            let d2 = dt2
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap();
            match d1.cmp(&d2) {
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Greater => 1,
            }
        }
        Some("d" | "day" | "days") => {
            let d1 = dt1.date_naive();
            let d2 = dt2.date_naive();
            match d1.cmp(&d2) {
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Greater => 1,
            }
        }
        Some("m" | "month" | "months") => {
            let y1 = dt1.year() as i64 * 12 + dt1.month() as i64;
            let y2 = dt2.year() as i64 * 12 + dt2.month() as i64;
            if y1 == y2 {
                0
            } else if y1 < y2 {
                -1
            } else {
                1
            }
        }
        Some("y" | "yyyy" | "yy" | "year" | "years") => {
            let y1 = dt1.year();
            let y2 = dt2.year();
            if y1 == y2 {
                0
            } else if y1 < y2 {
                -1
            } else {
                1
            }
        }
        Some(other) => return Err(format!("dateCompare() invalid datepart: {}", other)),
    };
    Ok(BxValue::new_number(result as f64))
}

fn date_convert(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("dateConvert() expects 2 arguments: (conversionType, date)".to_string());
    }
    let conversion = vm.to_string(args[0]).trim().to_ascii_lowercase();
    let timezone = vm
        .resolve_variable_path("__default_timezone")
        .map(|value| vm.to_string(value));
    match conversion.as_str() {
        "utc2local" => {
            let dt = parse_datetime_input(&vm.to_string(args[1]), None, None)?;
            let id = vm.datetime_new_with_timezone(dt, timezone.as_deref().unwrap_or("UTC"));
            return Ok(BxValue::new_ptr(id));
        }
        "local2utc" => {
            let dt = parse_datetime_input(&vm.to_string(args[1]), None, timezone.as_deref())?;
            let id = vm.datetime_new_with_timezone(dt, "UTC");
            return Ok(BxValue::new_ptr(id));
        }
        _ => {
            return Err(format!(
                "dateConvert() invalid conversion type: {}. Use 'utc2Local' or 'local2Utc'",
                conversion
            ));
        }
    }
}

fn date_part(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("datePart() expects 2 arguments: (datepart, date)".to_string());
    }
    let part = vm.to_string(args[0]).trim().to_ascii_lowercase();
    let tz = args.get(2).map(|v| vm.to_string(*v)).or_else(|| {
        vm.resolve_variable_path("__default_timezone")
            .map(|v| vm.to_string(v))
    });
    let dt = parse_datetime_input(&vm.to_string(args[1]), None, tz.as_deref())?;

    let result = match part.as_str() {
        "yyyy" | "yy" | "year" | "years" => dt.year() as f64,
        "q" | "quarter" => ((dt.month0() / 3) + 1) as f64,
        "m" | "month" | "months" => dt.month() as f64,
        "d" | "day" | "days" => dt.day() as f64,
        "y" | "dayofyear" => dt.ordinal() as f64,
        "w" | "dayofweek" => (dt.weekday().num_days_from_sunday() + 1) as f64,
        "ww" | "week" | "weeks" => dt.iso_week().week() as f64,
        "h" | "hour" | "hours" => dt.hour() as f64,
        "n" | "minute" | "minutes" => dt.minute() as f64,
        "s" | "second" | "seconds" => dt.second() as f64,
        "l" | "millisecond" | "milliseconds" => (dt.nanosecond() / 1_000_000) as f64,
        _ => return Err(format!("datePart() invalid datepart: {}", part)),
    };
    Ok(BxValue::new_number(result))
}

fn get_timezone_info(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let tz_name = args.first().map(|v| vm.to_string(*v));
    let local_offset = *chrono::Local::now().offset();
    let local_seconds = local_offset.local_minus_utc();
    let (id, total_seconds, name, short_name, name_dst, short_name_dst) = match tz_name.as_deref() {
        Some("UTC") | Some("GMT") | Some("Z") => (
            tz_name.clone().unwrap_or_else(|| "UTC".to_string()),
            0,
            "Coordinated Universal Time".to_string(),
            "UTC".to_string(),
            "Coordinated Universal Time".to_string(),
            "UTC".to_string(),
        ),
        Some("US/Hawaii") => (
            "US/Hawaii".to_string(),
            -36_000,
            "Hawaii-Aleutian Standard Time".to_string(),
            "HST".to_string(),
            "Hawaii-Aleutian Daylight Time".to_string(),
            "HDT".to_string(),
        ),
        Some(requested) => (
            requested.to_string(),
            parse_timezone_offset(Some(requested))
                .map(|offset| offset.local_minus_utc())
                .unwrap_or(local_seconds),
            "Local".to_string(),
            "Local".to_string(),
            "Local".to_string(),
            "Local".to_string(),
        ),
        None => (
            format!(
                "UTC{}{:02}:{:02}",
                if local_seconds >= 0 { '+' } else { '-' },
                local_seconds.abs() / 3600,
                (local_seconds.abs() % 3600) / 60
            ),
            local_seconds,
            "Local".to_string(),
            "Local".to_string(),
            "Local".to_string(),
            "Local".to_string(),
        ),
    };
    let hour_offset = total_seconds / 3600;
    let minute_offset = (total_seconds.abs() % 3600) / 60;

    let s = vm.struct_new();
    let id_ptr = vm.string_new(id.clone());
    vm.struct_set(s, "id", BxValue::new_ptr(id_ptr));
    let tz_ptr = vm.string_new(id);
    vm.struct_set(s, "timezone", BxValue::new_ptr(tz_ptr));
    vm.struct_set(s, "offset", BxValue::new_number(total_seconds as f64));
    vm.struct_set(s, "utcHourOffset", BxValue::new_number(hour_offset as f64));
    vm.struct_set(
        s,
        "utcMinuteOffset",
        BxValue::new_number(if total_seconds < 0 {
            -(minute_offset as f64)
        } else {
            minute_offset as f64
        }),
    );
    vm.struct_set(
        s,
        "utcTotalOffset",
        BxValue::new_number(total_seconds.abs() as f64),
    );
    vm.struct_set(s, "isDSTon", BxValue::new_bool(false));
    vm.struct_set(s, "DSTOffset", BxValue::new_number(0.0));
    let name_ptr = vm.string_new(name);
    vm.struct_set(s, "name", BxValue::new_ptr(name_ptr));
    let short_ptr = vm.string_new(short_name);
    vm.struct_set(s, "shortName", BxValue::new_ptr(short_ptr));
    let name_dst_ptr = vm.string_new(name_dst);
    vm.struct_set(s, "nameDST", BxValue::new_ptr(name_dst_ptr));
    let short_dst_ptr = vm.string_new(short_name_dst);
    vm.struct_set(s, "shortNameDST", BxValue::new_ptr(short_dst_ptr));
    Ok(BxValue::new_ptr(s))
}

fn set_timezone(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("setTimezone() expects 1 argument".to_string());
    }
    let tz = vm.to_string(args[0]);
    let normalized = tz.to_ascii_lowercase();
    if parse_timezone_offset(Some(&normalized)).is_none() {
        return Err(format!("Unknown timezone '{}'", tz));
    }
    let ptr = vm.string_new(tz);
    vm.insert_global("__default_timezone".to_string(), BxValue::new_ptr(ptr));
    Ok(BxValue::new_null())
}

fn clear_timezone(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    vm.insert_global("__default_timezone".to_string(), BxValue::new_null());
    Ok(BxValue::new_null())
}

fn create_odbc_date_time(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("createODBCDateTime() expects at least 1 argument".to_string());
    }
    let tz = args.get(1).map(|v| vm.to_string(*v));
    let dt = parse_datetime_input(&vm.to_string(args[0]), None, tz.as_deref())?;
    let formatted = dt.format("{ts '%Y-%m-%d %H:%M:%S'}").to_string();
    let ptr = vm.string_new(formatted);
    Ok(BxValue::new_ptr(ptr))
}

fn create_odbc_date(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let value = args
        .first()
        .copied()
        .ok_or_else(|| "createODBCDate() expects a date".to_string())?;
    let dt = parse_datetime_input(&vm.to_string(value), None, None)?;
    Ok(BxValue::new_ptr(
        vm.string_new(dt.format("{d '%Y-%m-%d'}").to_string()),
    ))
}

fn create_odbc_time(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let value = args
        .first()
        .copied()
        .ok_or_else(|| "createODBCTime() expects a date".to_string())?;
    let dt = parse_datetime_input(&vm.to_string(value), None, None)?;
    Ok(BxValue::new_ptr(
        vm.string_new(dt.format("{t '%H:%M:%S'}").to_string()),
    ))
}

pub(super) fn time_units(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("timeUnits() expects at least 1 argument: (datepart, [date])".to_string());
    }
    let part = vm.to_string(args[0]).trim().to_ascii_lowercase();
    let requested_tz = args.get(2).map(|v| vm.to_string(*v));
    let dt = if args.len() >= 2 {
        if args[1].is_number() || args[1].is_int() {
            Utc.timestamp_millis_opt(0).single().unwrap()
                + chrono::Duration::milliseconds((args[1].as_number() * 86_400_000.0).round() as i64)
        } else {
            parse_datetime_input(&vm.to_string(args[1]), None, None)?
        }
    } else {
        Utc::now()
    };

    let result: BxValue = match part.as_str() {
        "year" | "yyyy" => BxValue::new_number(dt.year() as f64),
        "quarter" | "q" => BxValue::new_number(((dt.month0() / 3) + 1) as f64),
        "month" | "m" => BxValue::new_number(dt.month() as f64),
        "monthasstring" => {
            let names = [
                "January",
                "February",
                "March",
                "April",
                "May",
                "June",
                "July",
                "August",
                "September",
                "October",
                "November",
                "December",
            ];
            let name = names[(dt.month0()) as usize];
            let ptr = vm.string_new(name.to_string());
            BxValue::new_ptr(ptr)
        }
        "monthshortasstring" => {
            let s = dt.format("%b").to_string();
            let ptr = vm.string_new(s);
            BxValue::new_ptr(ptr)
        }
        "day" | "d" => BxValue::new_number(dt.day() as f64),
        "dayofweek" | "w" => {
            let dow = dt.weekday().num_days_from_sunday();
            BxValue::new_number((dow + 1) as f64)
        }
        "dayofweekasstring" => {
            let names = [
                "Sunday",
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
            ];
            let idx = dt.weekday().num_days_from_sunday() as usize;
            let ptr = vm.string_new(names[idx].to_string());
            BxValue::new_ptr(ptr)
        }
        "dayofweekshortasstring" => {
            let s = dt.format("%a").to_string();
            let ptr = vm.string_new(s);
            BxValue::new_ptr(ptr)
        }
        "daysinmonth" => {
            let month = dt.month();
            let year = dt.year();
            let days = match month {
                1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
                4 | 6 | 9 | 11 => 30,
                2 => {
                    let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
                    if leap { 29 } else { 28 }
                }
                _ => 30,
            };
            BxValue::new_number(days as f64)
        }
        "daysinyear" => {
            let year = dt.year();
            let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
            BxValue::new_number(if leap { 366.0 } else { 365.0 })
        }
        "dayofyear" | "y" => BxValue::new_number(dt.ordinal() as f64),
        "firstdayofmonth" => BxValue::new_number(1.0),
        "week" | "ww" => BxValue::new_number(dt.iso_week().week() as f64),
        "hour" | "h" => BxValue::new_number(dt.hour() as f64),
        "minute" | "n" => BxValue::new_number(dt.minute() as f64),
        "second" | "s" => BxValue::new_number(dt.second() as f64),
        "millisecond" | "l" => BxValue::new_number((dt.nanosecond() / 1_000_000) as f64),
        "nanosecond" => BxValue::new_number(dt.nanosecond() as f64),
        "offset" => {
            let offset = requested_tz
                .as_deref()
                .and_then(|tz| parse_timezone_offset(Some(tz)))
                .unwrap_or_else(|| parse_timezone_offset(Some("UTC")).unwrap());
            let seconds = offset.local_minus_utc();
            let sign = if seconds < 0 { '-' } else { '+' };
            let absolute = seconds.unsigned_abs();
            let offset_str = format!(
                "{}{:02}:{:02}",
                sign,
                absolute / 3600,
                (absolute % 3600) / 60
            );
            let ptr = vm.string_new(offset_str);
            BxValue::new_ptr(ptr)
        }
        "timezone" | "gettimezone" => {
            let timezone = requested_tz
                .or_else(|| {
                    vm.resolve_variable_path("__default_timezone")
                        .map(|v| vm.to_string(v))
                })
                .unwrap_or_else(|| "UTC".to_string());
            let ptr = vm.string_new(timezone);
            BxValue::new_ptr(ptr)
        }
        "getnumericdate" => {
            let epoch = DateTime::<Utc>::from_naive_utc_and_offset(
                NaiveDate::from_ymd_opt(1970, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
                Utc,
            );
            let epoch_days = dt.signed_duration_since(epoch).num_milliseconds() as f64 / 86400000.0;
            BxValue::new_number(epoch_days)
        }
        "gettime" => BxValue::new_number(dt.timestamp_millis() as f64),
        _ => return Err(format!("timeUnits() invalid datepart: {}", part)),
    };
    Ok(result)
}

macro_rules! time_unit_bif {
    ($name:ident, $part:literal) => {
        fn $name(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
            let part = BxValue::new_ptr(vm.string_new($part.to_string()));
            let mut forwarded = vec![part];
            forwarded.extend_from_slice(args);
            time_units(vm, &forwarded)
        }
    };
}

time_unit_bif!(year_bif, "year");
time_unit_bif!(quarter_bif, "quarter");
time_unit_bif!(month_bif, "month");
time_unit_bif!(month_as_string_bif, "monthasstring");
time_unit_bif!(month_short_as_string_bif, "monthshortasstring");
time_unit_bif!(day_bif, "day");
time_unit_bif!(days_in_month_bif, "daysinmonth");
time_unit_bif!(days_in_year_bif, "daysinyear");
time_unit_bif!(day_of_week_bif, "dayofweek");
time_unit_bif!(day_of_week_as_string_bif, "dayofweekasstring");
time_unit_bif!(day_of_week_short_as_string_bif, "dayofweekshortasstring");
time_unit_bif!(day_of_year_bif, "dayofyear");
time_unit_bif!(first_day_of_month_bif, "firstdayofmonth");
time_unit_bif!(week_bif, "week");
time_unit_bif!(hour_bif, "hour");
time_unit_bif!(minute_bif, "minute");
time_unit_bif!(second_bif, "second");
time_unit_bif!(millisecond_bif, "millisecond");
time_unit_bif!(nanosecond_bif, "nanosecond");
