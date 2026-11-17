//! The canvas: immediate-mode drawing (fill, text, gradient text, bordered boxes) straight onto the terminal buffer,
//! in call order, with full styles and true colour. A frame is a list of `DrawCmd`; `render` paints it onto a `Buffer`,
//! so a UI can be drawn on a real terminal or into memory (for tests) by the same code.
use crate::color;
use crate::imaging::{self, Fit, KittyPlacement, Protocol};
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
    /// A picture from a file inside a rectangle of cells (see `imaging`). With `Protocol::HalfBlocks` it is painted into the
    /// cells; with `Protocol::Kitty` it is handed to the terminal as a placement (and painted as half blocks when `render`
    /// is not given anywhere to put placements, as in `render_to_string`).
    Image { x: u16, y: u16, w: u16, h: u16, path: String, fit: Fit, protocol: Protocol },
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

/// Paints the commands onto the buffer, in order. Anything outside the buffer is clipped. Every picture is painted as
/// half blocks, which is what a test or `render_to_string` reads back.
pub fn render(cmds: &[DrawCmd], buf: &mut Buffer) {
    render_with(cmds, buf, None);
}

/// The two stacked pixels of a half-block cell: `▀` has the top pixel as the foreground and the bottom one as the background.
fn paint_half_blocks(buf: &mut Buffer, x: u16, y: u16, w: u16, h: u16, path: &str, fit: Fit) {
    let Some((key, img)) = imaging::load(path) else { return };
    let Some(grid) = imaging::grid_for(&key, &img, w, h, fit) else { return };
    for cy in 0..h {
        for cx in 0..w {
            let (px, py) = (x as i32 + cx as i32, y as i32 + cy as i32);
            if !inside(buf, px, py) {
                continue;
            }
            let (top, bottom) = (grid.pixel(cx, 2 * cy), grid.pixel(cx, 2 * cy + 1));
            let rgb = |c: (u8, u8, u8)| Color::Rgb(c.0, c.1, c.2);
            let cell = &mut buf[(px as u16, py as u16)];
            match (top, bottom) {
                (None, None) => {}
                (Some(t), Some(b)) => {
                    cell.reset();
                    cell.set_char('▀').set_fg(rgb(t)).set_bg(rgb(b));
                }
                (Some(t), None) => {
                    cell.set_char('▀').set_fg(rgb(t));
                }
                (None, Some(b)) => {
                    cell.set_char('▄').set_fg(rgb(b));
                }
            }
        }
    }
}

/// Like `render`, but a picture whose protocol is Kitty is not painted into the buffer: it is pushed to `placements` for the
/// caller to send to the terminal. With `placements` as `None` every picture is painted as half blocks.
pub fn render_with(cmds: &[DrawCmd], buf: &mut Buffer, mut placements: Option<&mut Vec<KittyPlacement>>) {
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
            DrawCmd::Image { x, y, w, h, path, fit, protocol } => {
                if *w == 0 || *h == 0 || *w > imaging::MAX_CELLS || *h > imaging::MAX_CELLS {
                    continue;
                }
                match (protocol, placements.as_deref_mut()) {
                    (Protocol::Kitty, Some(list)) => {
                        // A panic while working out the placement leaves it out; the frame goes on.
                        let _ = imaging::guard(|| {
                            imaging::fault_point("place");
                            let Some((key, img)) = imaging::load(path) else { return };
                            let (cols, rows, dx, dy, crop) = match fit {
                                Fit::Contain => {
                                    let b = imaging::contain_box(img.width(), img.height(), *w, *h);
                                    (b.cols, b.rows, b.dx, b.dy, None)
                                }
                                Fit::Cover => (*w, *h, 0, 0, Some(imaging::cover_crop(img.width(), img.height(), *w, *h))),
                            };
                            let (px, py) = (x.saturating_add(dx), y.saturating_add(dy));
                            // Only the part that is on the screen is placed.
                            if px < buf.area.width && py < buf.area.height {
                                let cols = cols.min(buf.area.width - px);
                                let rows = rows.min(buf.area.height - py);
                                list.push(KittyPlacement { key, x: px, y: py, cols, rows, crop });
                            }
                        });
                    }
                    // A panic while painting leaves the picture out (or half painted); the canvas stays usable.
                    _ => {
                        let _ = imaging::guard(|| paint_half_blocks(buf, *x, *y, *w, *h, path, *fit));
                    }
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

    // ---- pictures ----
    use image::{Rgba, RgbaImage};

    /// Writes a PNG to a temporary file and returns its path.
    fn png(name: &str, img: &RgbaImage) -> String {
        let dir = std::env::temp_dir().join(format!("matchbox-tui-image-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        img.save(&path).unwrap();
        path.to_string_lossy().into_owned()
    }
    fn image_cmd(x: u16, y: u16, w: u16, h: u16, path: &str, fit: Fit, protocol: Protocol) -> DrawCmd {
        DrawCmd::Image { x, y, w, h, path: path.to_string(), fit, protocol }
    }
    /// 2 x 4 pixels: red, green / blue, white / black, yellow / cyan, magenta (two cell rows of two cells).
    fn four_rows() -> RgbaImage {
        let mut img = RgbaImage::new(2, 4);
        let colours = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 255], [0, 0, 0], [255, 255, 0], [0, 255, 255], [255, 0, 255]];
        for (i, c) in colours.iter().enumerate() {
            img.put_pixel((i % 2) as u32, (i / 2) as u32, Rgba([c[0], c[1], c[2], 255]));
        }
        img
    }

    #[test]
    fn a_picture_is_painted_as_half_blocks_with_the_top_pixel_in_the_foreground() {
        let path = png("four-rows.png", &four_rows());
        let buf = canvas(5, 3, &[image_cmd(1, 0, 2, 2, &path, Fit::Contain, Protocol::HalfBlocks)]);
        assert_eq!(rows_text(&buf), vec![" \u{2580}\u{2580}  ", " \u{2580}\u{2580}  ", "     "]);
        // cell (1,0): top pixel red, bottom pixel blue; cell (2,0): green over white
        assert_eq!((buf[(1, 0)].fg, buf[(1, 0)].bg), (Color::Rgb(255, 0, 0), Color::Rgb(0, 0, 255)));
        assert_eq!((buf[(2, 0)].fg, buf[(2, 0)].bg), (Color::Rgb(0, 255, 0), Color::Rgb(255, 255, 255)));
        // second cell row: black over cyan, yellow over magenta
        assert_eq!((buf[(1, 1)].fg, buf[(1, 1)].bg), (Color::Rgb(0, 0, 0), Color::Rgb(0, 255, 255)));
        assert_eq!((buf[(2, 1)].fg, buf[(2, 1)].bg), (Color::Rgb(255, 255, 0), Color::Rgb(255, 0, 255)));
        assert_eq!(buf[(0, 0)].symbol(), " ", "cells outside the rectangle are untouched");
    }

    #[test]
    fn contain_leaves_the_rest_of_the_rectangle_alone_and_cover_fills_it() {
        let mut wide = RgbaImage::new(4, 1);
        for x in 0..4 {
            wide.put_pixel(x, 0, Rgba([200, 100, 50, 255]));
        }
        let path = png("wide.png", &wide);
        let cells = |buf: &Buffer| (0..4u16).flat_map(|y| (0..4u16).map(move |x| (x, y))).filter(|p| buf[*p].symbol() != " ").count();
        let contain = canvas(4, 4, &[image_cmd(0, 0, 4, 4, &path, Fit::Contain, Protocol::HalfBlocks)]);
        assert_eq!(cells(&contain), 4, "one row of four cells: {:?}", rows_text(&contain));
        let cover = canvas(4, 4, &[image_cmd(0, 0, 4, 4, &path, Fit::Cover, Protocol::HalfBlocks)]);
        assert_eq!(cells(&cover), 16, "every cell");
    }

    #[test]
    fn transparent_pixels_leave_the_cell_as_it_was() {
        let mut img = RgbaImage::new(1, 2);
        img.put_pixel(0, 0, Rgba([255, 0, 0, 255]));
        img.put_pixel(0, 1, Rgba([0, 0, 0, 0]));
        let path = png("half-clear.png", &img);
        let buf = canvas(1, 1, &[
            DrawCmd::Fill { x: 0, y: 0, w: 1, h: 1, style: st(None, Some("#292c3c")) },
            image_cmd(0, 0, 1, 1, &path, Fit::Contain, Protocol::HalfBlocks),
        ]);
        assert_eq!(buf[(0, 0)].symbol(), "\u{2580}");
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(255, 0, 0));
        assert_eq!(buf[(0, 0)].bg, Color::Rgb(0x29, 0x2c, 0x3c), "the bottom half shows what was there");
    }

    #[test]
    fn a_missing_or_broken_file_draws_nothing() {
        let junk = std::env::temp_dir().join(format!("matchbox-tui-junk-{}.png", std::process::id()));
        std::fs::write(&junk, b"this is not a picture").unwrap();
        let buf = canvas(3, 2, &[
            image_cmd(0, 0, 3, 2, "/nonexistent/nope.png", Fit::Contain, Protocol::HalfBlocks),
            image_cmd(0, 0, 3, 2, junk.to_str().unwrap(), Fit::Cover, Protocol::HalfBlocks),
        ]);
        assert_eq!(rows_text(&buf), vec!["   ", "   "]);
    }

    #[test]
    fn a_picture_that_runs_off_the_canvas_is_clipped() {
        let path = png("clip.png", &four_rows());
        let buf = canvas(3, 2, &[image_cmd(2, 1, 2, 2, &path, Fit::Contain, Protocol::HalfBlocks)]);
        assert_eq!(rows_text(&buf), vec!["   ", "  \u{2580}"]);
        assert_eq!(buf[(2, 1)].fg, Color::Rgb(255, 0, 0));
        let nothing = canvas(3, 2, &[image_cmd(50, 50, 2, 2, &path, Fit::Contain, Protocol::HalfBlocks), image_cmd(0, 0, 0, 0, &path, Fit::Contain, Protocol::HalfBlocks)]);
        assert_eq!(rows_text(&nothing), vec!["   ", "   "]);
    }

    #[test]
    fn text_drawn_after_a_picture_lands_on_top_and_text_before_it_is_covered() {
        let path = png("text-over.png", &four_rows());
        let buf = canvas(4, 2, &[
            DrawCmd::Text { x: 0, y: 0, text: "ab".into(), style: CellStyle::default() },
            image_cmd(0, 0, 2, 2, &path, Fit::Contain, Protocol::HalfBlocks),
            DrawCmd::Text { x: 1, y: 1, text: "Zq".into(), style: st(Some("#00ff78"), Some("#303446")) },
        ]);
        assert_eq!(buf[(0, 0)].symbol(), "\u{2580}", "text before the picture is covered by it");
        assert_eq!(rows_text(&buf)[1], "\u{2580}Zq ");
        assert_eq!(buf[(1, 1)].fg, Color::Rgb(0, 255, 120));
        assert_eq!(buf[(1, 1)].bg, Color::Rgb(48, 52, 70));
        assert_eq!(buf[(3, 0)].symbol(), " ");
    }

    #[test]
    fn a_kitty_picture_becomes_a_placement_and_leaves_the_cells_alone() {
        let path = png("kitty.png", &four_rows());
        let cmds = [image_cmd(2, 1, 6, 3, &path, Fit::Contain, Protocol::Kitty)];
        // render_to_string and plain render paint every picture as half blocks
        let plain = canvas(10, 5, &cmds);
        assert_ne!(plain[(3, 1)].symbol(), " ", "drawn where the placement would be");
        assert_eq!(plain[(2, 1)].symbol(), " ");
        // the real terminal path hands it over instead
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 5));
        let mut placements = Vec::new();
        render_with(&cmds, &mut buf, Some(&mut placements));
        assert_eq!(rows_text(&buf).concat().trim(), "");
        assert_eq!(placements.len(), 1);
        let p = &placements[0];
        // a 2 x 4 picture in 6 x 3 cells (6 x 6 pixels): 3 pixels wide? contain makes it 3 x 6 pixels = 3 cells x 3 rows, centred
        assert_eq!((p.cols, p.rows), (3, 3));
        assert_eq!((p.x, p.y), (3, 1));
        assert_eq!(p.crop, None);
        // half-block drawing is unaffected by Kitty being the detected protocol when placements are not wanted
        let cover = [image_cmd(0, 0, 4, 2, &path, Fit::Cover, Protocol::Kitty)];
        let mut buf2 = Buffer::empty(Rect::new(0, 0, 10, 5));
        let mut p2 = Vec::new();
        render_with(&cover, &mut buf2, Some(&mut p2));
        assert_eq!(p2[0].crop.is_some(), true);
        assert_eq!((p2[0].cols, p2[0].rows), (4, 2));
    }

    #[test]
    fn a_panic_while_drawing_or_placing_a_picture_leaves_it_out_and_the_next_frame_draws() {
        let path = png("faults.png", &four_rows());
        let cmds = [image_cmd(0, 0, 2, 2, &path, Fit::Contain, Protocol::HalfBlocks), DrawCmd::Text { x: 4, y: 0, text: "ok".into(), style: CellStyle::default() }];
        imaging::inject_fault("draw");
        let broken = canvas(6, 2, &cmds);
        assert_eq!(rows_text(&broken), vec!["    ok", "      "], "no picture, the rest of the frame is intact");
        let fine = canvas(6, 2, &cmds);
        assert_eq!(rows_text(&fine)[0], "\u{2580}\u{2580}  ok");
        // the Kitty path: a panic while working out the placement leaves it out; the next frame places it
        let kitty = [image_cmd(1, 1, 4, 2, &path, Fit::Contain, Protocol::Kitty)];
        imaging::inject_fault("place");
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 4));
        let mut list = Vec::new();
        render_with(&kitty, &mut buf, Some(&mut list));
        assert!(list.is_empty());
        render_with(&kitty, &mut buf, Some(&mut list));
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn an_absurdly_large_rectangle_is_refused_and_a_position_near_the_limit_does_not_overflow() {
        let path = png("rect.png", &four_rows());
        let buf = canvas(4, 2, &[image_cmd(0, 0, 60000, 60000, &path, Fit::Contain, Protocol::HalfBlocks)]);
        assert_eq!(rows_text(&buf), vec!["    ", "    "]);
        let mut b = Buffer::empty(Rect::new(0, 0, 4, 2));
        let mut list = Vec::new();
        render_with(&[image_cmd(65535, 65535, 4, 4, &path, Fit::Contain, Protocol::Kitty)], &mut b, Some(&mut list));
        assert!(list.is_empty(), "off the screen: nothing to place, and no overflow");
    }
}
