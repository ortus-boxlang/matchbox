//! Colours for the terminal UI: names, `#RRGGBB`, `#RGB` and `rgb(r, g, b)`, with an automatic fallback to the
//! 256-colour palette when the terminal does not advertise true colour.
use ratatui::buffer::Buffer;
use ratatui::style::Color;

/// Parses a colour. Accepted: the 16 ANSI names (`red`, `lightblue`, `gray`, ...), `default` or `reset`,
/// `#RRGGBB`, `#RGB` and `rgb(r, g, b)`. Anything else is `None`.
pub fn parse(color: &str) -> Option<Color> {
    let text = color.trim();
    let lower = text.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(inner) = lower.strip_prefix("rgb(").and_then(|rest| rest.strip_suffix(')')) {
        let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
        if let [r, g, b] = parts[..] {
            return Some(Color::Rgb(r.parse().ok()?, g.parse().ok()?, b.parse().ok()?));
        }
        return None;
    }
    Some(match lower.as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "default" | "reset" => Color::Reset,
        _ => return None,
    })
}

/// Parses a colour, or returns white for text that is not a colour (the behaviour the widgets always had).
pub fn parse_or_white(color: &str) -> Color {
    parse(color).unwrap_or(Color::White)
}

fn parse_hex(hex: &str) -> Option<Color> {
    let digits: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|d| d as u8)).collect::<Option<Vec<_>>>()?;
    match digits[..] {
        [r, g, b] => Some(Color::Rgb(r * 17, g * 17, b * 17)),
        [r1, r2, g1, g2, b1, b2] => Some(Color::Rgb(r1 * 16 + r2, g1 * 16 + g2, b1 * 16 + b2)),
        _ => None,
    }
}

/// Blends two colours; t = 0 gives `a`, t = 1 gives `b`. Only RGB colours blend; any other colour returns `a`.
pub fn lerp(a: Color, b: Color, t: f32) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            let t = t.clamp(0.0, 1.0);
            let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
            Color::Rgb(mix(ar, br), mix(ag, bg), mix(ab, bb))
        }
        _ => a,
    }
}

/// True when the terminal says it can show 24-bit colour (`COLORTERM=truecolor` or `24bit`).
pub fn terminal_has_truecolor() -> bool {
    matches!(std::env::var("COLORTERM").map(|v| v.to_ascii_lowercase()).as_deref(), Ok("truecolor") | Ok("24bit"))
}

/// The nearest entry of the 256-colour palette (the 6x6x6 cube or the grey ramp) for an RGB colour.
pub fn to_indexed(r: u8, g: u8, b: u8) -> u8 {
    let level = |v: u8| -> u8 {
        if v < 48 { 0 } else if v < 115 { 1 } else { ((v as u16 - 35) / 40) as u8 }
    };
    let (cr, cg, cb) = (level(r), level(g), level(b));
    let cube = 16 + 36 * cr + 6 * cg + cb;
    let cube_rgb = |c: u8| if c == 0 { 0 } else { 55 + 40 * c as i32 };
    let dist = |x: (i32, i32, i32), y: (i32, i32, i32)| (x.0 - y.0).pow(2) + (x.1 - y.1).pow(2) + (x.2 - y.2).pow(2);
    let want = (r as i32, g as i32, b as i32);
    let cube_d = dist(want, (cube_rgb(cr), cube_rgb(cg), cube_rgb(cb)));
    let avg = (r as i32 + g as i32 + b as i32) / 3;
    let grey_step = ((avg - 8).max(0) / 10).min(23);
    let grey_v = 8 + 10 * grey_step;
    let grey_d = dist(want, (grey_v, grey_v, grey_v));
    if grey_d < cube_d { (232 + grey_step) as u8 } else { cube }
}

/// Rewrites every RGB colour in the buffer to the 256-colour palette. Used when the terminal has no true colour.
pub fn downgrade(buf: &mut Buffer) {
    let area = buf.area;
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            if let Color::Rgb(r, g, b) = cell.fg {
                cell.set_fg(Color::Indexed(to_indexed(r, g, b)));
            }
            if let Color::Rgb(r, g, b) = cell.bg {
                cell.set_bg(Color::Indexed(to_indexed(r, g, b)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn names_hex_and_rgb_all_parse() {
        assert_eq!(parse("red"), Some(Color::Red));
        assert_eq!(parse("LightBlue"), Some(Color::LightBlue));
        assert_eq!(parse("default"), Some(Color::Reset));
        assert_eq!(parse("#00FF78"), Some(Color::Rgb(0, 255, 120)));
        assert_eq!(parse("#0f8"), Some(Color::Rgb(0, 255, 136)));
        assert_eq!(parse("rgb(48, 52, 70)"), Some(Color::Rgb(48, 52, 70)));
        assert_eq!(parse("  #303446 "), Some(Color::Rgb(48, 52, 70)));
    }

    #[test]
    fn things_that_are_not_colours_do_not_parse() {
        assert_eq!(parse("chartreuse"), None);
        assert_eq!(parse("#12"), None);
        assert_eq!(parse("#12345g"), None);
        assert_eq!(parse("rgb(1,2)"), None);
        assert_eq!(parse("rgb(300,0,0)"), None);
        assert_eq!(parse_or_white("nonsense"), Color::White);
    }

    #[test]
    fn lerp_blends_rgb_and_leaves_other_colours() {
        assert_eq!(lerp(Color::Rgb(0, 0, 0), Color::Rgb(200, 100, 50), 0.5), Color::Rgb(100, 50, 25));
        assert_eq!(lerp(Color::Rgb(0, 0, 0), Color::Rgb(200, 100, 50), 2.0), Color::Rgb(200, 100, 50));
        assert_eq!(lerp(Color::Red, Color::Rgb(1, 2, 3), 0.5), Color::Red);
    }

    #[test]
    fn rgb_maps_to_the_nearest_256_colour() {
        assert_eq!(to_indexed(0, 0, 0), 16);
        assert_eq!(to_indexed(255, 255, 255), 231);
        assert_eq!(to_indexed(255, 0, 0), 196);
        assert_eq!(to_indexed(0, 255, 0), 46);
        // a mid grey lands on the grey ramp, not the cube
        assert!((232..=255).contains(&to_indexed(128, 128, 128)));
    }

    #[test]
    fn downgrade_rewrites_only_rgb_cells() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        buf[(0, 0)].set_fg(Color::Rgb(255, 0, 0)).set_bg(Color::Rgb(0, 0, 0));
        buf[(1, 0)].set_fg(Color::Red);
        downgrade(&mut buf);
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(196));
        assert_eq!(buf[(0, 0)].bg, Color::Indexed(16));
        assert_eq!(buf[(1, 0)].fg, Color::Red);
    }
}
