//! The canvas: immediate-mode drawing (fill, text, gradient text, bordered boxes) straight onto the terminal buffer,
//! in call order, with full styles and true colour. A frame is a list of `DrawCmd`; `render` paints it onto a `Buffer`,
//! so a UI can be drawn on a real terminal or into memory (for tests) by the same code.
use crate::color;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

/// Colours and attributes for one drawing command. `None` leaves what is already in the cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CellStyle {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

impl CellStyle {
    pub fn to_style(&self) -> Style {
        let mut style = Style::default();
        if let Some(fg) = self.fg {
            style = style.fg(fg);
        }
        if let Some(bg) = self.bg {
            style = style.bg(bg);
        }
        let mut mods = Modifier::empty();
        if self.bold {
            mods |= Modifier::BOLD;
        }
        if self.dim {
            mods |= Modifier::DIM;
        }
        if self.italic {
            mods |= Modifier::ITALIC;
        }
        if self.underline {
            mods |= Modifier::UNDERLINED;
        }
        style.add_modifier(mods)
    }
}

/// The border characters for `border`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderStyle {
    Plain,
    Rounded,
    Double,
    Thick,
}

impl BorderStyle {
    /// `plain`, `rounded`, `double` or `thick`; anything else is `rounded`.
    pub fn parse(name: &str) -> BorderStyle {
        match name.to_ascii_lowercase().as_str() {
            "plain" | "single" => BorderStyle::Plain,
            "double" => BorderStyle::Double,
            "thick" => BorderStyle::Thick,
            _ => BorderStyle::Rounded,
        }
    }

    /// (top-left, top-right, bottom-left, bottom-right, horizontal, vertical)
    fn glyphs(self) -> (char, char, char, char, char, char) {
        match self {
            BorderStyle::Plain => ('┌', '┐', '└', '┘', '─', '│'),
            BorderStyle::Rounded => ('╭', '╮', '╰', '╯', '─', '│'),
            BorderStyle::Double => ('╔', '╗', '╚', '╝', '═', '║'),
            BorderStyle::Thick => ('┏', '┓', '┗', '┛', '━', '┃'),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCmd {
    /// Paint a rectangle with the style's background (and clear its text).
    Fill { x: u16, y: u16, w: u16, h: u16, style: CellStyle },
    /// Write text at a position.
    Text { x: u16, y: u16, text: String, style: CellStyle },
    /// Write text with the foreground blending from `from` to `to` across its width.
    Gradient { x: u16, y: u16, text: String, from: Color, to: Color, style: CellStyle },
    /// A bordered rectangle; the inside is filled when the style has a background. `title` sits on the top edge.
    Border { x: u16, y: u16, w: u16, h: u16, border: BorderStyle, style: CellStyle, title: String, title_style: CellStyle },
}

fn inside(buf: &Buffer, x: i32, y: i32) -> bool {
    let a = buf.area;
    x >= a.x as i32 && y >= a.y as i32 && x < (a.x + a.width) as i32 && y < (a.y + a.height) as i32
}

fn put(buf: &mut Buffer, x: i32, y: i32, ch: char, style: Style) {
    if inside(buf, x, y) {
        buf[(x as u16, y as u16)].set_char(ch).set_style(style);
    }
}

fn write_text(buf: &mut Buffer, x: u16, y: u16, text: &str, style: Style) {
    for (i, ch) in text.chars().enumerate() {
        put(buf, x as i32 + i as i32, y as i32, ch, style);
    }
}

/// Paints the commands onto the buffer, in order. Anything outside the buffer is clipped.
pub fn render(cmds: &[DrawCmd], buf: &mut Buffer) {
    for cmd in cmds {
        match cmd {
            DrawCmd::Fill { x, y, w, h, style } => {
                let st = style.to_style();
                for yy in *y as i32..*y as i32 + *h as i32 {
                    for xx in *x as i32..*x as i32 + *w as i32 {
                        put(buf, xx, yy, ' ', st);
                    }
                }
            }
            DrawCmd::Text { x, y, text, style } => write_text(buf, *x, *y, text, style.to_style()),
            DrawCmd::Gradient { x, y, text, from, to, style } => {
                let n = text.chars().count().max(1);
                for (i, ch) in text.chars().enumerate() {
                    let t = if n == 1 { 0.0 } else { i as f32 / (n - 1) as f32 };
                    let mut cell = style.clone();
                    cell.fg = Some(color::lerp(*from, *to, t));
                    put(buf, *x as i32 + i as i32, *y as i32, ch, cell.to_style());
                }
            }
            DrawCmd::Border { x, y, w, h, border, style, title, title_style } => {
                if *w < 2 || *h < 2 {
                    continue;
                }
                let st = style.to_style();
                let (tl, tr, bl, br, hz, vt) = border.glyphs();
                let (x0, y0) = (*x as i32, *y as i32);
                let (x1, y1) = (x0 + *w as i32 - 1, y0 + *h as i32 - 1);
                if style.bg.is_some() {
                    for yy in y0 + 1..y1 {
                        for xx in x0 + 1..x1 {
                            put(buf, xx, yy, ' ', st);
                        }
                    }
                }
                for xx in x0 + 1..x1 {
                    put(buf, xx, y0, hz, st);
                    put(buf, xx, y1, hz, st);
                }
                for yy in y0 + 1..y1 {
                    put(buf, x0, yy, vt, st);
                    put(buf, x1, yy, vt, st);
                }
                put(buf, x0, y0, tl, st);
                put(buf, x1, y0, tr, st);
                put(buf, x0, y1, bl, st);
                put(buf, x1, y1, br, st);
                if !title.is_empty() && *w > 6 {
                    let room = (*w as usize).saturating_sub(6);
                    let shown: String = title.chars().take(room).collect();
                    let mut merged = style.clone();
                    if title_style.fg.is_some() {
                        merged.fg = title_style.fg;
                    }
                    if title_style.bg.is_some() {
                        merged.bg = title_style.bg;
                    }
                    merged.bold |= title_style.bold;
                    write_text(buf, *x + 2, *y, &format!(" {} ", shown), merged.to_style());
                }
            }
        }
    }
}

/// The plain characters of a buffer, one string per row (trailing spaces kept), for tests and snapshots.
pub fn rows_text(buf: &Buffer) -> Vec<String> {
    let a: Rect = buf.area;
    (a.y..a.y + a.height).map(|y| (a.x..a.x + a.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(fg: Option<&str>, bg: Option<&str>) -> CellStyle {
        CellStyle { fg: fg.and_then(color::parse), bg: bg.and_then(color::parse), ..CellStyle::default() }
    }
    fn canvas(w: u16, h: u16, cmds: &[DrawCmd]) -> Buffer {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        render(cmds, &mut buf);
        buf
    }

    #[test]
    fn text_is_written_with_its_style_and_clipped_to_the_buffer() {
        let buf = canvas(6, 2, &[DrawCmd::Text { x: 4, y: 1, text: "hello".into(), style: CellStyle { bold: true, ..st(Some("#00ff78"), Some("#303446")) } }]);
        assert_eq!(rows_text(&buf), vec!["      ", "    he"]);
        assert_eq!(buf[(4, 1)].fg, Color::Rgb(0, 255, 120));
        assert_eq!(buf[(5, 1)].bg, Color::Rgb(48, 52, 70));
        assert!(buf[(4, 1)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn fill_paints_a_rectangle_and_clears_text_under_it() {
        let buf = canvas(5, 3, &[
            DrawCmd::Text { x: 0, y: 1, text: "abcde".into(), style: CellStyle::default() },
            DrawCmd::Fill { x: 1, y: 0, w: 3, h: 2, style: st(None, Some("#292c3c")) },
        ]);
        assert_eq!(rows_text(&buf), vec!["     ", "a   e", "     "]);
        assert_eq!(buf[(2, 1)].bg, Color::Rgb(0x29, 0x2c, 0x3c));
        assert_eq!(buf[(0, 1)].bg, Color::Reset);
    }

    #[test]
    fn gradient_text_blends_from_the_first_colour_to_the_last() {
        let buf = canvas(5, 1, &[DrawCmd::Gradient { x: 0, y: 0, text: "abcde".into(), from: Color::Rgb(0, 219, 255), to: Color::Rgb(0, 255, 120), style: CellStyle::default() }]);
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(0, 219, 255));
        assert_eq!(buf[(4, 0)].fg, Color::Rgb(0, 255, 120));
        assert_ne!(buf[(2, 0)].fg, buf[(0, 0)].fg);
    }

    #[test]
    fn a_rounded_box_has_corners_edges_a_title_and_a_filled_inside() {
        let buf = canvas(14, 4, &[DrawCmd::Border {
            x: 0, y: 0, w: 14, h: 4, border: BorderStyle::Rounded,
            style: st(Some("#51576d"), Some("#292c3c")), title: "Steps".into(), title_style: st(Some("#00ff78"), None),
        }]);
        assert_eq!(rows_text(&buf), vec!["╭─ Steps ────╮", "│            │", "│            │", "╰────────────╯"]);
        assert_eq!(buf[(3, 0)].fg, Color::Rgb(0, 255, 120));
        assert_eq!(buf[(1, 1)].bg, Color::Rgb(0x29, 0x2c, 0x3c));
    }

    #[test]
    fn border_styles_use_their_own_glyphs() {
        for (name, corner, edge) in [("plain", '┌', '─'), ("double", '╔', '═'), ("thick", '┏', '━'), ("anything", '╭', '─')] {
            let buf = canvas(4, 3, &[DrawCmd::Border { x: 0, y: 0, w: 4, h: 3, border: BorderStyle::parse(name), style: CellStyle::default(), title: String::new(), title_style: CellStyle::default() }]);
            assert_eq!(buf[(0, 0)].symbol().chars().next(), Some(corner), "{name}");
            assert_eq!(buf[(1, 0)].symbol().chars().next(), Some(edge), "{name}");
        }
    }

    #[test]
    fn commands_apply_in_order_and_drawing_outside_the_buffer_is_ignored() {
        let buf = canvas(3, 1, &[
            DrawCmd::Text { x: 0, y: 0, text: "abc".into(), style: CellStyle::default() },
            DrawCmd::Text { x: 1, y: 0, text: "XY".into(), style: CellStyle::default() },
            DrawCmd::Text { x: 9, y: 9, text: "gone".into(), style: CellStyle::default() },
            DrawCmd::Fill { x: 2, y: 0, w: 50, h: 50, style: CellStyle::default() },
        ]);
        assert_eq!(rows_text(&buf), vec!["aX "]);
    }
}
