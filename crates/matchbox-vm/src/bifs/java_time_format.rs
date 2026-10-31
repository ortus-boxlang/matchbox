//! Mirrors `java.time.format.DateTimeFormatter` pattern formatting as used by the
//! BoxLang `dateFormat`/`timeFormat`/`dateTimeFormat` BIFs (`DateTimeFormat.java`).
//!
//! BoxLang passes the mask straight to `DateTimeFormatterBuilder.appendPattern`, so
//! the pattern language here is the Java one: case-sensitive letters, `mm` is minutes,
//! `MM` is month, invalid letters and repeat counts raise the same errors as Java.
//! Locale text and localized patterns come from `java_time_locale.rs`, generated from
//! the JDK. Parity is verified against `tests/fixtures/datetime_format/golden.tsv`.

use super::java_time_locale::{JavaLocale, LOCALES};
use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};

/// The zone a value is formatted in. `region` zones keep their id (`UTC`, `GMT`,
/// `America/New_York`); offset zones are ids such as `+05:30` or `Z`.
#[derive(Clone, Debug)]
pub struct JavaZone {
    pub id: String,
    pub region: bool,
    pub offset_secs: i32,
}

impl JavaZone {
    pub fn utc() -> Self {
        JavaZone {
            id: "UTC".to_string(),
            region: true,
            offset_secs: 0,
        }
    }
}

/// A local date-time plus the zone it is displayed in.
#[derive(Clone, Debug)]
pub struct ZonedFields {
    pub local: NaiveDateTime,
    pub zone: JavaZone,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Date,
    Time,
    DateTime,
}

/// Resolve a BoxLang locale string (`de`, `DE`, `de-DE`, `de_DE`) to a supported locale.
/// Languages without generated data fall back to `en_US`.
pub fn find_locale(tag: Option<&str>) -> &'static JavaLocale {
    let en_us = &LOCALES[0];
    let Some(tag) = tag.map(str::trim).filter(|t| !t.is_empty()) else {
        return en_us;
    };
    let mut parts = tag.split(['-', '_', ' ']);
    let lang = parts.next().unwrap_or("").to_ascii_lowercase();
    let country = parts.next().map(str::to_ascii_uppercase);
    if let Some(country) = &country {
        let wanted = format!("{}_{}", lang, country);
        if let Some(found) = LOCALES.iter().find(|l| l.tag == wanted) {
            return found;
        }
    }
    LOCALES
        .iter()
        .find(|l| l.tag.split('_').next() == Some(lang.as_str()))
        .unwrap_or(en_us)
}

// ---------------------------------------------------------------------------
// Pattern compilation
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Field {
    Era,
    YearOfEra,
    Year,
    Quarter,
    Month,
    DayOfYear,
    DayOfMonth,
    AlignedWeekOfMonth,
    DayOfWeek,
    AmPm,
    HourOfDay,
    ClockHourOfDay,
    HourOfAmPm,
    ClockHourOfAmPm,
    Minute,
    Second,
    Nano,
    MilliOfDay,
    NanoOfDay,
    ModifiedJulianDay,
    // locale week fields
    LocalizedDayOfWeek,
    WeekOfMonth,
    WeekOfWeekBasedYear,
    WeekBasedYear,
}

impl Field {
    /// `ChronoField` / `IsoFields` display names used in Java error messages.
    fn name(self) -> &'static str {
        match self {
            Field::Era => "Era",
            Field::YearOfEra => "YearOfEra",
            Field::Year => "Year",
            Field::Quarter => "QuarterOfYear",
            Field::Month => "MonthOfYear",
            Field::DayOfYear => "DayOfYear",
            Field::DayOfMonth => "DayOfMonth",
            Field::AlignedWeekOfMonth => "AlignedWeekOfMonth",
            Field::DayOfWeek => "DayOfWeek",
            Field::AmPm => "AmPmOfDay",
            Field::HourOfDay => "HourOfDay",
            Field::ClockHourOfDay => "ClockHourOfDay",
            Field::HourOfAmPm => "HourOfAmPm",
            Field::ClockHourOfAmPm => "ClockHourOfAmPm",
            Field::Minute => "MinuteOfHour",
            Field::Second => "SecondOfMinute",
            Field::Nano => "NanoOfSecond",
            Field::MilliOfDay => "MilliOfDay",
            Field::NanoOfDay => "NanoOfDay",
            Field::ModifiedJulianDay => "ModifiedJulianDay",
            Field::LocalizedDayOfWeek => "DayOfWeek[WeekFields]",
            Field::WeekOfMonth => "WeekOfMonth[WeekFields]",
            Field::WeekOfWeekBasedYear => "WeekOfWeekBasedYear[WeekFields]",
            Field::WeekBasedYear => "WeekBasedYear[WeekFields]",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Sign {
    Normal,
    NotNegative,
    ExceedsPad,
}

/// Index into the locale text tables: short, full, narrow (+3 for standalone).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TextStyle {
    Short,
    Full,
    Narrow,
    ShortStandalone,
    FullStandalone,
    NarrowStandalone,
}

impl TextStyle {
    fn index(self) -> usize {
        match self {
            TextStyle::Short => 0,
            TextStyle::Full => 1,
            TextStyle::Narrow => 2,
            TextStyle::ShortStandalone => 3,
            TextStyle::FullStandalone => 4,
            TextStyle::NarrowStandalone => 5,
        }
    }

    fn base(self) -> usize {
        self.index() % 3
    }
}

#[derive(Clone, Debug)]
enum Elem {
    Lit(String),
    Num {
        field: Field,
        min: usize,
        max: usize,
        sign: Sign,
    },
    Reduced {
        field: Field,
    },
    Fraction {
        width: usize,
    },
    Text {
        field: Field,
        style: TextStyle,
    },
    Offset {
        pattern: &'static str,
        zero: &'static str,
    },
    LocalizedOffset {
        full: bool,
    },
    ZoneId,
    ZoneText {
        full: bool,
        generic: bool,
    },
    DayPeriod {
        style: usize,
    },
    Pad {
        width: usize,
        inner: Box<Elem>,
    },
    Optional(Vec<Elem>),
}

const OFFSET_PATTERNS: [&str; 22] = [
    "+HH",
    "+HHmm",
    "+HH:mm",
    "+HHMM",
    "+HH:MM",
    "+HHMMss",
    "+HH:MM:ss",
    "+HHMMSS",
    "+HH:MM:SS",
    "+HHmmss",
    "+HH:mm:ss",
    "+H",
    "+Hmm",
    "+H:mm",
    "+HMM",
    "+H:MM",
    "+HMMss",
    "+H:MM:ss",
    "+HMMSS",
    "+H:MM:SS",
    "+Hmmss",
    "+H:mm:ss",
];

fn field_for(c: char) -> Option<Field> {
    Some(match c {
        'G' => Field::Era,
        'y' => Field::YearOfEra,
        'u' => Field::Year,
        'Q' | 'q' => Field::Quarter,
        'M' | 'L' => Field::Month,
        'D' => Field::DayOfYear,
        'd' => Field::DayOfMonth,
        'F' => Field::AlignedWeekOfMonth,
        'E' | 'e' | 'c' => Field::DayOfWeek,
        'a' => Field::AmPm,
        'H' => Field::HourOfDay,
        'k' => Field::ClockHourOfDay,
        'K' => Field::HourOfAmPm,
        'h' => Field::ClockHourOfAmPm,
        'm' => Field::Minute,
        's' => Field::Second,
        'S' | 'n' => Field::Nano,
        'A' => Field::MilliOfDay,
        'N' => Field::NanoOfDay,
        'g' => Field::ModifiedJulianDay,
        _ => return None,
    })
}

fn num(field: Field, min: usize, max: usize, sign: Sign) -> Result<Elem, String> {
    if !(1..=19).contains(&min) {
        return Err(format!(
            "The minimum width must be from 1 to 19 inclusive but was {}",
            min
        ));
    }
    if !(1..=19).contains(&max) {
        return Err(format!(
            "The maximum width must be from 1 to 19 inclusive but was {}",
            max
        ));
    }
    if max < min {
        return Err(format!(
            "The maximum width must exceed or equal the minimum width but {} < {}",
            max, min
        ));
    }
    Ok(Elem::Num {
        field,
        min,
        max,
        sign,
    })
}

fn fixed(field: Field, width: usize) -> Result<Elem, String> {
    if !(1..=19).contains(&width) {
        return Err(format!(
            "The width must be from 1 to 19 inclusive but was {}",
            width
        ));
    }
    Ok(Elem::Num {
        field,
        min: width,
        max: width,
        sign: Sign::NotNegative,
    })
}

fn too_many(c: char) -> String {
    format!("Too many pattern letters: {}", c)
}

fn parse_field(c: char, count: usize, field: Field) -> Result<Elem, String> {
    let mut standalone = false;
    match c {
        'u' | 'y' => {
            if count == 2 {
                return Ok(Elem::Reduced { field });
            }
            return if count < 4 {
                num(field, count, 19, Sign::Normal)
            } else {
                num(field, count, 19, Sign::ExceedsPad)
            };
        }
        'c' | 'L' | 'q' | 'M' | 'Q' | 'E' | 'e' => {
            if c == 'c' && count == 2 {
                return Err("Invalid pattern \"cc\"".to_string());
            }
            if matches!(c, 'c' | 'L' | 'q') {
                standalone = true;
            }
        }
        _ => {}
    }
    match c {
        'c' | 'L' | 'q' | 'M' | 'Q' | 'E' | 'e' => match count {
            1 => {
                if c == 'c' || c == 'e' {
                    Ok(Elem::Num {
                        field: Field::LocalizedDayOfWeek,
                        min: 1,
                        max: 1,
                        sign: Sign::NotNegative,
                    })
                } else if c == 'E' {
                    Ok(Elem::Text {
                        field,
                        style: TextStyle::Short,
                    })
                } else {
                    num(field, 1, 19, Sign::Normal)
                }
            }
            2 => {
                if c == 'c' || c == 'e' {
                    Ok(Elem::Num {
                        field: Field::LocalizedDayOfWeek,
                        min: 2,
                        max: 2,
                        sign: Sign::NotNegative,
                    })
                } else if c == 'E' {
                    Ok(Elem::Text {
                        field,
                        style: TextStyle::Short,
                    })
                } else {
                    fixed(field, 2)
                }
            }
            3 => Ok(Elem::Text {
                field: if matches!(c, 'c' | 'e') {
                    Field::DayOfWeek
                } else {
                    field
                },
                style: if standalone {
                    TextStyle::ShortStandalone
                } else {
                    TextStyle::Short
                },
            }),
            4 => Ok(Elem::Text {
                field: if matches!(c, 'c' | 'e') {
                    Field::DayOfWeek
                } else {
                    field
                },
                style: if standalone {
                    TextStyle::FullStandalone
                } else {
                    TextStyle::Full
                },
            }),
            5 => Ok(Elem::Text {
                field: if matches!(c, 'c' | 'e') {
                    Field::DayOfWeek
                } else {
                    field
                },
                style: if standalone {
                    TextStyle::NarrowStandalone
                } else {
                    TextStyle::Narrow
                },
            }),
            _ => Err(too_many(c)),
        },
        'a' => {
            if count == 1 {
                Ok(Elem::Text {
                    field,
                    style: TextStyle::Short,
                })
            } else {
                Err(too_many(c))
            }
        }
        'G' => match count {
            1..=3 => Ok(Elem::Text {
                field,
                style: TextStyle::Short,
            }),
            4 => Ok(Elem::Text {
                field,
                style: TextStyle::Full,
            }),
            5 => Ok(Elem::Text {
                field,
                style: TextStyle::Narrow,
            }),
            _ => Err(too_many(c)),
        },
        'S' => {
            if count > 9 {
                return Err(format!(
                    "Minimum width must be from 0 to 9 inclusive but was {}",
                    count
                ));
            }
            Ok(Elem::Fraction { width: count })
        }
        'F' => {
            if count == 1 {
                num(field, 1, 19, Sign::Normal)
            } else {
                Err(too_many(c))
            }
        }
        'd' | 'h' | 'H' | 'k' | 'K' | 'm' | 's' => match count {
            1 => num(field, 1, 19, Sign::Normal),
            2 => fixed(field, 2),
            _ => Err(too_many(c)),
        },
        'D' => match count {
            1 => num(field, 1, 19, Sign::Normal),
            2 | 3 => num(field, count, 3, Sign::NotNegative),
            _ => Err(too_many(c)),
        },
        'g' => num(field, count, 19, Sign::Normal),
        'A' | 'n' | 'N' => num(field, count, 19, Sign::NotNegative),
        _ => {
            if count == 1 {
                num(field, 1, 19, Sign::Normal)
            } else {
                fixed(field, count)
            }
        }
    }
}

fn compile(pattern: &str) -> Result<Vec<Elem>, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut stack: Vec<Vec<Elem>> = vec![Vec::new()];
    let mut pos = 0;
    while pos < chars.len() {
        let mut cur = chars[pos];
        if cur.is_ascii_alphabetic() {
            let mut start = pos;
            pos += 1;
            while pos < chars.len() && chars[pos] == cur {
                pos += 1;
            }
            let mut count = pos - start;
            let mut pad = 0usize;
            if cur == 'p' {
                if pos < chars.len() && chars[pos].is_ascii_alphabetic() {
                    pad = count;
                    cur = chars[pos];
                    start = pos;
                    pos += 1;
                    while pos < chars.len() && chars[pos] == cur {
                        pos += 1;
                    }
                    count = pos - start;
                }
                if pad == 0 {
                    return Err(format!(
                        "Pad letter 'p' must be followed by valid pad pattern: {}",
                        pattern
                    ));
                }
            }
            let elem = if let Some(field) = field_for(cur) {
                parse_field(cur, count, field)?
            } else {
                match cur {
                    'z' => {
                        if count > 4 {
                            return Err(too_many(cur));
                        }
                        Elem::ZoneText {
                            full: count == 4,
                            generic: false,
                        }
                    }
                    'V' => {
                        if count != 2 {
                            return Err(format!("Pattern letter count must be 2: {}", cur));
                        }
                        Elem::ZoneId
                    }
                    'v' => match count {
                        1 => Elem::ZoneText {
                            full: false,
                            generic: true,
                        },
                        4 => Elem::ZoneText {
                            full: true,
                            generic: true,
                        },
                        _ => return Err(format!("Wrong number of pattern letters: {}", cur)),
                    },
                    'Z' => match count {
                        1..=3 => Elem::Offset {
                            pattern: "+HHMM",
                            zero: "+0000",
                        },
                        4 => Elem::LocalizedOffset { full: true },
                        5 => Elem::Offset {
                            pattern: "+HH:MM:ss",
                            zero: "Z",
                        },
                        _ => return Err(too_many(cur)),
                    },
                    'O' => match count {
                        1 => Elem::LocalizedOffset { full: false },
                        4 => Elem::LocalizedOffset { full: true },
                        _ => return Err(format!("Pattern letter count must be 1 or 4: {}", cur)),
                    },
                    'X' => {
                        if count > 5 {
                            return Err(too_many(cur));
                        }
                        Elem::Offset {
                            pattern: OFFSET_PATTERNS[count + usize::from(count != 1)],
                            zero: "Z",
                        }
                    }
                    'x' => {
                        if count > 5 {
                            return Err(too_many(cur));
                        }
                        let zero = if count == 1 {
                            "+00"
                        } else if count % 2 == 0 {
                            "+0000"
                        } else {
                            "+00:00"
                        };
                        Elem::Offset {
                            pattern: OFFSET_PATTERNS[count + usize::from(count != 1)],
                            zero,
                        }
                    }
                    'W' => {
                        if count > 1 {
                            return Err(too_many(cur));
                        }
                        Elem::Num {
                            field: Field::WeekOfMonth,
                            min: count,
                            max: count,
                            sign: Sign::NotNegative,
                        }
                    }
                    'w' => {
                        if count > 2 {
                            return Err(too_many(cur));
                        }
                        Elem::Num {
                            field: Field::WeekOfWeekBasedYear,
                            min: count,
                            max: 2,
                            sign: Sign::NotNegative,
                        }
                    }
                    'Y' => {
                        if count == 2 {
                            Elem::Reduced {
                                field: Field::WeekBasedYear,
                            }
                        } else {
                            Elem::Num {
                                field: Field::WeekBasedYear,
                                min: count,
                                max: 19,
                                sign: if count < 4 {
                                    Sign::Normal
                                } else {
                                    Sign::ExceedsPad
                                },
                            }
                        }
                    }
                    'B' => match count {
                        1 => Elem::DayPeriod { style: 0 },
                        4 => Elem::DayPeriod { style: 1 },
                        5 => Elem::DayPeriod { style: 2 },
                        _ => return Err(format!("Wrong number of pattern letters: {}", cur)),
                    },
                    _ => return Err(format!("Unknown pattern letter: {}", cur)),
                }
            };
            let elem = if pad > 0 {
                Elem::Pad {
                    width: pad,
                    inner: Box::new(elem),
                }
            } else {
                elem
            };
            stack.last_mut().unwrap().push(elem);
            continue;
        }
        match cur {
            '\'' => {
                let start = pos;
                pos += 1;
                let mut literal = String::new();
                let mut closed = false;
                while pos < chars.len() {
                    if chars[pos] == '\'' {
                        if pos + 1 < chars.len() && chars[pos + 1] == '\'' {
                            literal.push('\'');
                            pos += 1;
                        } else {
                            closed = true;
                            break;
                        }
                    } else {
                        literal.push(chars[pos]);
                    }
                    pos += 1;
                }
                if !closed {
                    return Err(format!(
                        "Pattern ends with an incomplete string literal: {}",
                        pattern
                    ));
                }
                let _ = start;
                if literal.is_empty() {
                    literal.push('\'');
                }
                stack.last_mut().unwrap().push(Elem::Lit(literal));
            }
            '[' => stack.push(Vec::new()),
            ']' => {
                if stack.len() == 1 {
                    return Err("Pattern invalid as it contains ] without previous [".to_string());
                }
                let inner = stack.pop().unwrap();
                stack.last_mut().unwrap().push(Elem::Optional(inner));
            }
            '{' | '}' | '#' => {
                return Err(format!("Pattern includes reserved character: '{}'", cur));
            }
            other => stack.last_mut().unwrap().push(Elem::Lit(other.to_string())),
        }
        pos += 1;
    }
    while stack.len() > 1 {
        let inner = stack.pop().unwrap();
        stack.last_mut().unwrap().push(Elem::Optional(inner));
    }
    Ok(stack.pop().unwrap())
}

// ---------------------------------------------------------------------------
// Formatting
// ---------------------------------------------------------------------------

struct Ctx<'a> {
    t: &'a ZonedFields,
    loc: &'static JavaLocale,
}

fn week_start_offset(day: i64, dow: i64, minimal_days: i64) -> i64 {
    let week_start = (day - dow).rem_euclid(7);
    let mut offset = -week_start;
    if week_start + 1 > minimal_days {
        offset = 7 - week_start;
    }
    offset
}

fn compute_week(offset: i64, day: i64) -> i64 {
    (7 + offset + (day - 1)) / 7
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

impl Ctx<'_> {
    fn localized_dow(&self, date: NaiveDate) -> i64 {
        let iso = date.weekday().number_from_monday() as i64;
        (iso - self.loc.first_day_of_week as i64).rem_euclid(7) + 1
    }

    /// Week-of-week-based-year and week-based-year for a date, per `WeekFields`.
    fn week_based(&self, date: NaiveDate) -> (i64, i64) {
        let min_days = self.loc.minimal_days as i64;
        let year = date.year() as i64;
        let dow = self.localized_dow(date);
        let doy = date.ordinal() as i64;
        let offset = week_start_offset(doy, dow, min_days);
        let week = compute_week(offset, doy);
        if week == 0 {
            let prev = date - chrono::Duration::days(doy);
            let (w, _) = self.week_based(prev);
            (w, year - 1)
        } else if week > 50 {
            let year_len = if is_leap(year) { 366 } else { 365 };
            let new_year_week = compute_week(offset, year_len + min_days);
            if week >= new_year_week {
                (week - new_year_week + 1, year + 1)
            } else {
                (week, year)
            }
        } else {
            (week, year)
        }
    }

    fn value(&self, field: Field) -> i64 {
        let dt = &self.t.local;
        let date = dt.date();
        let year = date.year() as i64;
        match field {
            Field::Era => i64::from(year >= 1),
            Field::YearOfEra => {
                if year >= 1 {
                    year
                } else {
                    1 - year
                }
            }
            Field::Year => year,
            Field::Quarter => (date.month0() / 3 + 1) as i64,
            Field::Month => date.month() as i64,
            Field::DayOfYear => date.ordinal() as i64,
            Field::DayOfMonth => date.day() as i64,
            Field::AlignedWeekOfMonth => ((date.day() as i64) - 1) / 7 + 1,
            Field::DayOfWeek => date.weekday().number_from_monday() as i64,
            Field::AmPm => i64::from(dt.hour() >= 12),
            Field::HourOfDay => dt.hour() as i64,
            Field::ClockHourOfDay => {
                if dt.hour() == 0 {
                    24
                } else {
                    dt.hour() as i64
                }
            }
            Field::HourOfAmPm => (dt.hour() % 12) as i64,
            Field::ClockHourOfAmPm => {
                let h = dt.hour() % 12;
                if h == 0 { 12 } else { h as i64 }
            }
            Field::Minute => dt.minute() as i64,
            Field::Second => dt.second() as i64,
            Field::Nano => dt.nanosecond() as i64,
            Field::MilliOfDay => {
                (dt.hour() as i64 * 3600 + dt.minute() as i64 * 60 + dt.second() as i64) * 1000
                    + (dt.nanosecond() / 1_000_000) as i64
            }
            Field::NanoOfDay => {
                (dt.hour() as i64 * 3600 + dt.minute() as i64 * 60 + dt.second() as i64)
                    * 1_000_000_000
                    + dt.nanosecond() as i64
            }
            Field::ModifiedJulianDay => {
                let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
                (date - epoch).num_days() + 40587
            }
            Field::LocalizedDayOfWeek => self.localized_dow(date),
            Field::WeekOfMonth => {
                let dom = date.day() as i64;
                let offset =
                    week_start_offset(dom, self.localized_dow(date), self.loc.minimal_days as i64);
                compute_week(offset, dom)
            }
            Field::WeekOfWeekBasedYear => self.week_based(date).0,
            Field::WeekBasedYear => self.week_based(date).1,
        }
    }

    fn text(&self, field: Field, style: TextStyle) -> String {
        let loc = self.loc;
        let v = self.value(field);
        match field {
            Field::Era => loc.eras[v as usize][style.base()].to_string(),
            Field::Month => loc.months[style.index()][(v - 1) as usize].to_string(),
            Field::DayOfWeek => loc.weekdays[style.index()][(v - 1) as usize].to_string(),
            Field::Quarter => loc.quarters[style.index()][(v - 1) as usize].to_string(),
            Field::AmPm => loc.am_pm[v as usize].to_string(),
            _ => v.to_string(),
        }
    }
}

fn format_number(
    field: Field,
    value: i64,
    min: usize,
    max: usize,
    sign: Sign,
) -> Result<String, String> {
    let digits = value.unsigned_abs().to_string();
    if digits.len() > max {
        return Err(format!(
            "Field {} cannot be printed as the value {} exceeds the maximum print width of {}",
            field.name(),
            value,
            max
        ));
    }
    let mut out = String::new();
    if value >= 0 {
        if sign == Sign::ExceedsPad && min < 19 && value >= 10i64.pow(min as u32) {
            out.push('+');
        }
    } else {
        match sign {
            Sign::NotNegative => {
                return Err(format!(
                    "Field {} cannot be printed as the value {} cannot be negative according to the SignStyle",
                    field.name(),
                    value
                ));
            }
            _ => out.push('-'),
        }
    }
    for _ in digits.len()..min {
        out.push('0');
    }
    out.push_str(&digits);
    Ok(out)
}

fn format_offset(pattern: &str, zero: &str, total: i32) -> String {
    if total == 0 {
        return zero.to_string();
    }
    let abs = total.abs();
    let hours = abs / 3600;
    let minutes = (abs / 60) % 60;
    let seconds = abs % 60;
    let colon = pattern.contains(':');
    let mut out = String::new();
    out.push(if total < 0 { '-' } else { '+' });
    // hour: "HH" two digits, "H" minimal
    if pattern.starts_with("+HH") {
        out.push_str(&format!("{:02}", hours));
    } else {
        out.push_str(&hours.to_string());
    }
    let tail: Vec<char> = pattern
        .trim_start_matches("+HH")
        .trim_start_matches("+H")
        .chars()
        .filter(|c| *c != ':')
        .collect();
    let minute_kind = tail.first().copied();
    let second_kind = tail.get(2).copied();
    let mut printed_minutes = false;
    if let Some(kind) = minute_kind {
        let optional = kind == 'm';
        // "+HHmm" style optional minutes are omitted when zero (and when seconds are zero)
        if !optional || minutes != 0 || (second_kind.is_some() && seconds != 0) {
            if colon {
                out.push(':');
            }
            out.push_str(&format!("{:02}", minutes));
            printed_minutes = true;
        }
    }
    if let Some(kind) = second_kind {
        let optional = kind == 's';
        if printed_minutes && (!optional || seconds != 0) {
            if colon {
                out.push(':');
            }
            out.push_str(&format!("{:02}", seconds));
        }
    }
    out
}

fn format_localized_offset(loc: &JavaLocale, full: bool, total: i32) -> String {
    if total == 0 {
        return loc.gmt_zero.to_string();
    }
    let abs = total.abs();
    let hours = abs / 3600;
    let minutes = (abs / 60) % 60;
    let seconds = abs % 60;
    let mut out = String::from(loc.gmt_prefix);
    out.push_str(if total < 0 { loc.gmt_minus } else { "+" });
    if full {
        out.push_str(&format!("{:02}:{:02}", hours, minutes));
        if seconds != 0 {
            out.push_str(&format!(":{:02}", seconds));
        }
    } else {
        out.push_str(&hours.to_string());
        if minutes != 0 || seconds != 0 {
            out.push_str(&format!(":{:02}", minutes));
            if seconds != 0 {
                out.push_str(&format!(":{:02}", seconds));
            }
        }
    }
    out
}

/// Names for the few regions MatchBox can place without a tz database. Everything else
/// falls back to the zone id (documented gap: no tz database).
fn region_text(zone: &JavaZone, full: bool, generic: bool) -> Option<&'static str> {
    let table: &[(&str, [&str; 4])] = &[
        (
            "America/New_York",
            ["EDT", "ET", "Eastern Daylight Time", "Eastern Time"],
        ),
        (
            "America/Chicago",
            ["CDT", "CT", "Central Daylight Time", "Central Time"],
        ),
        (
            "America/Denver",
            ["MDT", "MT", "Mountain Daylight Time", "Mountain Time"],
        ),
        (
            "America/Los_Angeles",
            ["PDT", "PT", "Pacific Daylight Time", "Pacific Time"],
        ),
        (
            "US/Hawaii",
            [
                "HST",
                "HST",
                "Hawaii-Aleutian Standard Time",
                "Hawaii-Aleutian Time",
            ],
        ),
        (
            "Pacific/Honolulu",
            [
                "HST",
                "HST",
                "Hawaii-Aleutian Standard Time",
                "Hawaii-Aleutian Time",
            ],
        ),
        (
            "Europe/London",
            ["BST", "UK", "British Summer Time", "United Kingdom Time"],
        ),
        (
            "Europe/Berlin",
            [
                "CEST",
                "Germany Time",
                "Central European Summer Time",
                "Germany Time",
            ],
        ),
        (
            "Europe/Zurich",
            [
                "CEST",
                "Switzerland Time",
                "Central European Summer Time",
                "Switzerland Time",
            ],
        ),
        (
            "Asia/Tokyo",
            ["JST", "Japan Time", "Japan Standard Time", "Japan Time"],
        ),
        (
            "Australia/Sydney",
            [
                "AEST",
                "Sydney Time",
                "Australian Eastern Standard Time",
                "Sydney Time",
            ],
        ),
    ];
    table
        .iter()
        .find(|(id, _)| *id == zone.id)
        .map(|(_, names)| names[usize::from(generic) + 2 * usize::from(full)])
}

fn zone_text(loc: &JavaLocale, zone: &JavaZone, full: bool, generic: bool) -> String {
    if !zone.region {
        return zone.id.clone();
    }
    match zone.id.as_str() {
        "UTC" | "Etc/UTC" => {
            loc.utc_names[usize::from(generic) * 2 + usize::from(full)].to_string()
        }
        "GMT" => loc.gmt_names[usize::from(generic) * 2 + usize::from(full)].to_string(),
        _ => region_text(zone, full, generic)
            .map(str::to_string)
            .unwrap_or_else(|| zone.id.clone()),
    }
}

fn format_elem(elem: &Elem, ctx: &Ctx, out: &mut String) -> Result<(), String> {
    match elem {
        Elem::Lit(s) => out.push_str(s),
        Elem::Num {
            field,
            min,
            max,
            sign,
        } => {
            out.push_str(&format_number(
                *field,
                ctx.value(*field),
                *min,
                *max,
                *sign,
            )?);
        }
        Elem::Reduced { field } => {
            let value = ctx.value(*field);
            let reduced = if (2000..2100).contains(&value) {
                value % 100
            } else {
                value.abs() % 100
            };
            out.push_str(&format_number(*field, reduced, 2, 2, Sign::NotNegative)?);
        }
        Elem::Fraction { width } => {
            let nano = format!("{:09}", ctx.t.local.nanosecond());
            out.push_str(&nano[..*width]);
        }
        Elem::Text { field, style } => out.push_str(&ctx.text(*field, *style)),
        Elem::Offset { pattern, zero } => {
            out.push_str(&format_offset(pattern, zero, ctx.t.zone.offset_secs))
        }
        Elem::LocalizedOffset { full } => out.push_str(&format_localized_offset(
            ctx.loc,
            *full,
            ctx.t.zone.offset_secs,
        )),
        Elem::ZoneId => out.push_str(&ctx.t.zone.id),
        Elem::ZoneText { full, generic } => {
            out.push_str(&zone_text(ctx.loc, &ctx.t.zone, *full, *generic))
        }
        Elem::DayPeriod { style } => {
            let dt = &ctx.t.local;
            let minute_slot = usize::from(dt.minute() != 0);
            out.push_str(ctx.loc.day_periods[dt.hour() as usize][minute_slot][*style]);
        }
        Elem::Pad { width, inner } => {
            let mut text = String::new();
            format_elem(inner, ctx, &mut text)?;
            let len = text.chars().count();
            if len > *width {
                return Err(format!(
                    "Cannot print as output of {} characters exceeds pad width of {}",
                    len, width
                ));
            }
            for _ in len..*width {
                out.push(' ');
            }
            out.push_str(&text);
        }
        Elem::Optional(inner) => {
            for e in inner {
                format_elem(e, ctx, out)?;
            }
        }
    }
    Ok(())
}

fn run(elems: &[Elem], t: &ZonedFields, loc: &'static JavaLocale) -> Result<String, String> {
    let ctx = Ctx { t, loc };
    let mut out = String::new();
    for e in elems {
        format_elem(e, &ctx, &mut out)?;
    }
    Ok(out)
}

/// Format with a Java `DateTimeFormatter` pattern (raw output, before BoxLang's space cleanup).
pub fn format_pattern(
    pattern: &str,
    t: &ZonedFields,
    loc: &'static JavaLocale,
) -> Result<String, String> {
    run(&compile(pattern)?, t, loc)
}

// ---------------------------------------------------------------------------
// BoxLang masks
// ---------------------------------------------------------------------------

/// `DateTime.sanitizeStringSpaces`: trim, normalise non-breaking spaces, collapse runs.
pub fn sanitize_spaces(input: &str) -> String {
    let trimmed = input.trim_matches(|c: char| c <= ' ');
    let replaced: String = trimmed
        .chars()
        .map(|c| {
            if c == '\u{a0}' || c == '\u{202f}' {
                ' '
            } else {
                c
            }
        })
        .collect();
    let mut out = String::with_capacity(replaced.len());
    let mut last_space = false;
    for c in replaced.chars() {
        if c == ' ' {
            if !last_space {
                out.push(c);
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out
}

enum Common {
    Localized {
        date: Option<usize>,
        time: Option<usize>,
    },
    Pattern(&'static str),
    Wrapped {
        prefix: &'static str,
        pattern: &'static str,
        suffix: &'static str,
    },
    IsoDate,
    IsoTime,
    IsoOffsetDateTime,
}

/// `DateTime.COMMON_FORMATTERS`, keyed (case-insensitively) by `<name><Mode>`.
fn common_formatter(key: &str) -> Option<Common> {
    let key = key.to_ascii_lowercase();
    Some(match key.as_str() {
        "fulldatetime" => Common::Localized {
            date: Some(0),
            time: Some(0),
        },
        "longdatetime" => Common::Localized {
            date: Some(1),
            time: Some(1),
        },
        "mediumdatetime" => Common::Localized {
            date: Some(2),
            time: Some(2),
        },
        "shortdatetime" => Common::Localized {
            date: Some(3),
            time: Some(3),
        },
        "isodatetime" => Common::Pattern("yyyy-MM-dd'T'HH:mm:ssXXX"),
        "iso8601datetime" => Common::IsoOffsetDateTime,
        "odbcdatetime" => Common::Wrapped {
            prefix: "{ts '",
            pattern: "yyyy-MM-dd HH:mm:ss",
            suffix: "'}",
        },
        "odbcdate" => Common::Wrapped {
            prefix: "{d '",
            pattern: "yyyy-MM-dd",
            suffix: "'}",
        },
        "odbctime" => Common::Wrapped {
            prefix: "{t '",
            pattern: "HH:mm:ss",
            suffix: "'}",
        },
        "javascriptdatetime" => Common::Pattern("EEE MMM dd yyyy HH:mm:ss 'GMT'Z (zzzz)"),
        "fulldate" => Common::Localized {
            date: Some(0),
            time: None,
        },
        "longdate" => Common::Localized {
            date: Some(1),
            time: None,
        },
        "mediumdate" => Common::Localized {
            date: Some(2),
            time: None,
        },
        "shortdate" => Common::Localized {
            date: Some(3),
            time: None,
        },
        "isodate" | "iso8601date" => Common::IsoDate,
        "fulltime" => Common::Localized {
            date: None,
            time: Some(0),
        },
        "longtime" => Common::Localized {
            date: None,
            time: Some(1),
        },
        "mediumtime" => Common::Localized {
            date: None,
            time: Some(2),
        },
        "shorttime" => Common::Localized {
            date: None,
            time: Some(3),
        },
        "isotime" | "iso8601time" => Common::IsoTime,
        _ => return None,
    })
}

fn format_common(
    common: &Common,
    t: &ZonedFields,
    loc: &'static JavaLocale,
) -> Result<String, String> {
    match common {
        Common::Localized { date, time } => {
            let pattern = match (date, time) {
                (Some(d), Some(tm)) => loc.datetime_styles[*d][*tm],
                (Some(d), None) => loc.date_styles[*d],
                (None, Some(tm)) => loc.time_styles[*tm],
                (None, None) => unreachable!("localized formatter needs a date or time style"),
            };
            format_pattern(pattern, t, loc)
        }
        Common::Pattern(p) => format_pattern(p, t, loc),
        Common::Wrapped {
            prefix,
            pattern,
            suffix,
        } => Ok(format!(
            "{}{}{}",
            prefix,
            format_pattern(pattern, t, loc)?,
            suffix
        )),
        Common::IsoDate => {
            let ctx = Ctx { t, loc };
            let mut out =
                format_number(Field::Year, ctx.value(Field::Year), 4, 10, Sign::ExceedsPad)?;
            out.push_str(&format!("-{:02}-{:02}", t.local.month(), t.local.day()));
            out.push_str(&format_offset("+HH:MM:ss", "Z", t.zone.offset_secs));
            Ok(out)
        }
        Common::IsoTime => Ok(iso_time(t)),
        Common::IsoOffsetDateTime => {
            let ctx = Ctx { t, loc };
            let mut out =
                format_number(Field::Year, ctx.value(Field::Year), 4, 10, Sign::ExceedsPad)?;
            out.push_str(&format!("-{:02}-{:02}T", t.local.month(), t.local.day()));
            out.push_str(&iso_time(t));
            Ok(out)
        }
    }
}

fn iso_time(t: &ZonedFields) -> String {
    let nano = format!("{:09}", t.local.nanosecond());
    let trimmed = nano.trim_end_matches('0');
    let fraction = if trimmed.is_empty() {
        String::new()
    } else {
        format!(".{}", trimmed)
    };
    format!(
        "{:02}:{:02}:{:02}{}{}",
        t.local.hour(),
        t.local.minute(),
        t.local.second(),
        fraction,
        format_offset("+HH:MM:ss", "Z", t.zone.offset_secs)
    )
}

fn default_pattern(mode: Mode) -> &'static str {
    match mode {
        Mode::Date => "dd-MMM-yy",
        Mode::Time => "hh:mm a",
        Mode::DateTime => "dd-MMM-yyyy HH:mm:ss",
    }
}

/// Mirror of `DateTimeFormat._invoke` once the date, zone and locale are resolved.
pub fn format_mask(
    t: &ZonedFields,
    mode: Mode,
    mask: Option<&str>,
    loc: &'static JavaLocale,
) -> Result<String, String> {
    let mode_name = match mode {
        Mode::Date => "Date",
        Mode::Time => "Time",
        Mode::DateTime => "DateTime",
    };
    let raw = match mask {
        None => format_pattern(default_pattern(mode), t, loc)?,
        Some(mask) if mask.eq_ignore_ascii_case("epoch") => {
            return Ok(epoch_seconds(t).to_string());
        }
        Some(mask) if mask.eq_ignore_ascii_case("epochms") => {
            return Ok(epoch_millis(t).to_string());
        }
        Some(mask) => {
            let key = format!("{}{}", mask.trim_matches(|c: char| c <= ' '), mode_name);
            if let Some(common) = common_formatter(&key) {
                format_common(&common, t, loc)?
            } else if let Some(common) = common_formatter(&format!("{}DateTime", mask)) {
                // ZonedDateTime.format(locale, mask) looks the mask up as a date-time common mask
                format_common(&common, t, loc)?
            } else {
                format_pattern(mask, t, loc)?
            }
        }
    };
    Ok(sanitize_spaces(&raw))
}

fn epoch_seconds(t: &ZonedFields) -> i64 {
    t.local.and_utc().timestamp() - t.zone.offset_secs as i64
}

fn epoch_millis(t: &ZonedFields) -> i64 {
    t.local.and_utc().timestamp_millis() - t.zone.offset_secs as i64 * 1000
}

#[cfg(test)]
mod golden_tests {
    use super::*;
    use chrono::DateTime;

    fn parse_instant(s: &str) -> chrono::DateTime<chrono::Utc> {
        // Java prints years beyond 9999 and before 0 with a sign; RFC 3339 needs 4 digits.
        let (sign, body) = match s.as_bytes()[0] {
            b'+' => (1i32, &s[1..]),
            b'-' => (-1i32, &s[1..]),
            _ => (1i32, s),
        };
        let dash = body.find('-').unwrap();
        let year: i32 = sign * body[..dash].parse::<i32>().unwrap();
        let rest = &body[dash + 1..];
        let rest = rest.trim_end_matches('Z');
        let (date_part, time_part) = rest.split_once('T').unwrap();
        let mut d = date_part.split('-').map(|p| p.parse::<u32>().unwrap());
        let (month, day) = (d.next().unwrap(), d.next().unwrap());
        let (hms, frac) = match time_part.split_once('.') {
            Some((h, f)) => (h, f),
            None => (time_part, ""),
        };
        let mut h = hms.split(':').map(|p| p.parse::<u32>().unwrap());
        let (hh, mm, ss) = (h.next().unwrap(), h.next().unwrap(), h.next().unwrap_or(0));
        let nano: u32 = if frac.is_empty() {
            0
        } else {
            format!("{:0<9}", frac).parse().unwrap()
        };
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_nano_opt(hh, mm, ss, nano)
            .unwrap()
            .and_utc()
    }

    fn zone_for(id: &str) -> JavaZone {
        match id {
            "UTC" => JavaZone::utc(),
            "GMT" => JavaZone {
                id: "GMT".to_string(),
                region: true,
                offset_secs: 0,
            },
            "Z" | "+00:00" => JavaZone {
                id: "Z".to_string(),
                region: false,
                offset_secs: 0,
            },
            other => {
                let sign = if other.starts_with('-') { -1 } else { 1 };
                let hh: i32 = other[1..3].parse().unwrap();
                let mm: i32 = other[4..6].parse().unwrap();
                JavaZone {
                    id: other.to_string(),
                    region: false,
                    offset_secs: sign * (hh * 3600 + mm * 60),
                }
            }
        }
    }

    fn unescape(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('t') => out.push('\t'),
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn matches_jdk_golden_output() {
        let golden = include_str!("../../../../tests/fixtures/datetime_format/golden.tsv");
        let mut failures = Vec::new();
        let mut total = 0usize;
        for line in golden.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            assert_eq!(cols.len(), 6, "bad golden row: {}", line);
            total += 1;
            let instant = parse_instant(cols[0]);
            let zone = zone_for(cols[1]);
            let local = (instant + chrono::Duration::seconds(zone.offset_secs as i64)).naive_utc();
            let t = ZonedFields { local, zone };
            let loc = find_locale(Some(cols[2]));
            let mode = match cols[3] {
                "date" => Mode::Date,
                "time" => Mode::Time,
                _ => Mode::DateTime,
            };
            let mask = if cols[4] == "<null>" {
                None
            } else {
                Some(unescape(cols[4]))
            };
            let expected = unescape(cols[5]);
            let actual = match format_mask(&t, mode, mask.as_deref(), loc) {
                Ok(v) => v,
                Err(e) => format!("ERR {}", e),
            };
            if actual != expected {
                failures.push(format!(
                    "{} {} {} {} mask={:?}: expected {:?} got {:?}",
                    cols[0], cols[1], cols[2], cols[3], mask, expected, actual
                ));
            }
        }
        let _ = DateTime::<chrono::Utc>::MIN_UTC;
        assert!(
            failures.is_empty(),
            "{} of {} golden cases differ; first 40:\n{}",
            failures.len(),
            total,
            failures
                .iter()
                .take(40)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
