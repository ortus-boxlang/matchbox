//! Images on the canvas. Two ways to show a picture in a terminal:
//!
//! * **Half blocks** (`HalfBlocks`): every cell is two stacked pixels, drawn with `▀` (foreground = top pixel, background =
//!   bottom pixel) in true colour. It works in any terminal and is the only form `render_to_string` can read back, so tests
//!   see exactly what a plain terminal sees.
//! * **Kitty graphics** (`Kitty`): the picture is sent to the terminal once, then placed over a rectangle of cells with a
//!   short escape sequence. Kitty and Ghostty draw it at full resolution.
//!
//! The terminal is never asked what it supports (a reply would arrive on the input stream): `detect_from` looks at the
//! environment only. Sixel and iTerm2 are accepted as names and fall back to half blocks in this version.
use image::imageops::{self, FilterType};
use image::RgbaImage;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// How a picture is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Protocol {
    /// Two pixels per cell with `▀`; works everywhere.
    HalfBlocks,
    /// The Kitty graphics protocol (Kitty, Ghostty).
    Kitty,
}

impl Protocol {
    /// The name `image_support()` returns and the `protocol` option accepts: `kitty` or `halfblocks`.
    pub fn name(self) -> &'static str {
        match self {
            Protocol::HalfBlocks => "halfblocks",
            Protocol::Kitty => "kitty",
        }
    }
}

/// What the `protocol` option asks for. `auto` follows `detect_from`; `sixel` and `iterm2` are accepted names that draw as half blocks for now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Auto,
    Kitty,
    HalfBlocks,
    Sixel,
    Iterm2,
}

impl Choice {
    /// `auto`, `kitty`, `halfblocks`, `sixel` or `iterm2` (any case); anything else is `None`.
    pub fn parse(name: &str) -> Option<Choice> {
        match name.to_ascii_lowercase().as_str() {
            "auto" | "" => Some(Choice::Auto),
            "kitty" => Some(Choice::Kitty),
            "halfblocks" | "half-blocks" | "halfblock" => Some(Choice::HalfBlocks),
            "sixel" => Some(Choice::Sixel),
            "iterm2" | "iterm" => Some(Choice::Iterm2),
            _ => None,
        }
    }

    /// The protocol that is actually used: `auto` is the detected one, `sixel` and `iterm2` are not drawn yet and use half blocks.
    pub fn resolve(self, detected: Protocol) -> Protocol {
        match self {
            Choice::Auto => detected,
            Choice::Kitty => Protocol::Kitty,
            Choice::HalfBlocks | Choice::Sixel | Choice::Iterm2 => Protocol::HalfBlocks,
        }
    }
}

/// The protocol `auto` picks, from environment variables only (no query is sent to the terminal, so nothing can block):
/// `MATCHBOX_TUI_IMAGES` (`kitty` or `halfblocks`) wins; inside tmux or screen it is half blocks (graphics need passthrough);
/// Kitty and Ghostty (`KITTY_WINDOW_ID`, `GHOSTTY_RESOURCES_DIR`, `TERM_PROGRAM=ghostty`, `TERM=xterm-kitty` or `xterm-ghostty`) get Kitty;
/// everything else gets half blocks.
pub fn detect_from(get: &dyn Fn(&str) -> Option<String>) -> Protocol {
    if let Some(forced) = get("MATCHBOX_TUI_IMAGES").and_then(|v| Choice::parse(&v)) {
        if forced != Choice::Auto {
            return forced.resolve(Protocol::HalfBlocks);
        }
    }
    if get("TMUX").is_some() || get("STY").is_some() {
        return Protocol::HalfBlocks;
    }
    let term = get("TERM").unwrap_or_default().to_ascii_lowercase();
    let program = get("TERM_PROGRAM").unwrap_or_default().to_ascii_lowercase();
    if get("KITTY_WINDOW_ID").is_some() || get("GHOSTTY_RESOURCES_DIR").is_some() || program == "ghostty" || term == "xterm-kitty" || term == "xterm-ghostty" {
        return Protocol::Kitty;
    }
    Protocol::HalfBlocks
}

/// `detect_from` on the real environment.
pub fn detect() -> Protocol {
    detect_from(&|name| std::env::var(name).ok())
}

/// How a picture fills its rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fit {
    /// The whole picture, as large as fits, centred; the rest of the rectangle is left as it was.
    Contain,
    /// The rectangle is filled; the picture is centred and the overflow is cut off.
    Cover,
}

impl Fit {
    /// `contain` or `cover`; anything else is `contain`.
    pub fn parse(name: &str) -> Fit {
        if name.eq_ignore_ascii_case("cover") { Fit::Cover } else { Fit::Contain }
    }
}

/// Identifies a picture file as it was when loaded: its path and when it last changed, so an edited file is read again.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageKey {
    pub path: String,
    pub len: u64,
    pub modified_ns: u128,
}

thread_local! {
    static PICTURES: RefCell<HashMap<ImageKey, Rc<RgbaImage>>> = RefCell::new(HashMap::new());
    static GRIDS: RefCell<HashMap<(ImageKey, u16, u16, Fit), Rc<Grid>>> = RefCell::new(HashMap::new());
}

/// A picture file decoded (PNG, JPEG, GIF or WebP; the first frame of a GIF), or `None` when it is missing or not a picture.
/// Decoded pictures are kept, so drawing the same file every frame costs a lookup.
pub fn load(path: &str) -> Option<(ImageKey, Rc<RgbaImage>)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified_ns = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    let key = ImageKey { path: path.to_string(), len: meta.len(), modified_ns };
    if let Some(hit) = PICTURES.with(|p| p.borrow().get(&key).cloned()) {
        return Some((key, hit));
    }
    let decoded = image::open(path).ok()?.to_rgba8();
    if decoded.width() == 0 || decoded.height() == 0 {
        return None;
    }
    let rc = Rc::new(decoded);
    PICTURES.with(|p| {
        let mut p = p.borrow_mut();
        if p.len() >= 48 {
            p.clear();
        }
        p.insert(key.clone(), rc.clone());
    });
    Some((key, rc))
}

/// Where a picture lands inside a `w` x `h` cell rectangle: the size and offset in cells.
/// A cell is one pixel wide and two high for half blocks, so the rectangle is `w` x `2h` pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Cell offset from the rectangle's left edge.
    pub dx: u16,
    /// Cell offset from the rectangle's top edge.
    pub dy: u16,
    /// Width in cells.
    pub cols: u16,
    /// Height in cells.
    pub rows: u16,
}

/// The cell box a picture of `iw` x `ih` pixels takes inside `w` x `h` cells with `Contain` (the aspect of a cell is taken as 1:2).
pub fn contain_box(iw: u32, ih: u32, w: u16, h: u16) -> Placement {
    let (gw, gh) = (w as f64, 2.0 * h as f64);
    let s = (gw / iw as f64).min(gh / ih as f64);
    let ow = ((iw as f64 * s).round() as u32).clamp(1, w as u32);
    let oh = ((ih as f64 * s).round() as u32).clamp(1, 2 * h as u32);
    let rows = oh.div_ceil(2).min(h as u32);
    Placement { dx: ((w as u32 - ow) / 2) as u16, dy: (((2 * h as u32 - oh) / 2) / 2) as u16, cols: ow as u16, rows: rows as u16 }
}

/// A picture resampled to the half-block pixel grid of a rectangle: `w` columns and `2h` pixel rows. `None` is transparent
/// (outside the picture, or alpha under half).
#[derive(Debug, PartialEq)]
pub struct Grid {
    pub w: u16,
    pub h: u16,
    pixels: Vec<Option<(u8, u8, u8)>>,
}

impl Grid {
    /// The pixel at column `x` and pixel row `y` (two per cell row).
    pub fn pixel(&self, x: u16, y: u16) -> Option<(u8, u8, u8)> {
        if x >= self.w || y >= 2 * self.h {
            return None;
        }
        self.pixels[y as usize * self.w as usize + x as usize]
    }
}

fn opaque(p: &image::Rgba<u8>) -> Option<(u8, u8, u8)> {
    if p[3] < 128 { None } else { Some((p[0], p[1], p[2])) }
}

/// Resamples `img` to a `w` x `2h` pixel grid. With the size equal to the picture nothing is resampled, so a picture drawn
/// at its own pixel size appears pixel for pixel.
pub fn make_grid(img: &RgbaImage, w: u16, h: u16, fit: Fit) -> Grid {
    let (gw, gh) = (w as u32, 2 * h as u32);
    let mut pixels = vec![None; (gw * gh) as usize];
    let (iw, ih) = img.dimensions();
    if gw == 0 || gh == 0 || iw == 0 || ih == 0 {
        return Grid { w, h, pixels };
    }
    let scale = match fit {
        Fit::Contain => (gw as f64 / iw as f64).min(gh as f64 / ih as f64),
        Fit::Cover => (gw as f64 / iw as f64).max(gh as f64 / ih as f64),
    };
    let ow = ((iw as f64 * scale).round() as u32).max(1);
    let oh = ((ih as f64 * scale).round() as u32).max(1);
    let scaled;
    let src: &RgbaImage = if (ow, oh) == (iw, ih) {
        img
    } else {
        scaled = imageops::resize(img, ow, oh, FilterType::Triangle);
        &scaled
    };
    // Place the scaled picture centred on the grid; the part outside the grid is cut off.
    let off_x = (gw as i64 - ow as i64).div_euclid(2);
    let off_y = (gh as i64 - oh as i64).div_euclid(2);
    for gy in 0..gh as i64 {
        for gx in 0..gw as i64 {
            let (sx, sy) = (gx - off_x, gy - off_y);
            if sx >= 0 && sy >= 0 && (sx as u32) < ow && (sy as u32) < oh {
                pixels[(gy as u32 * gw + gx as u32) as usize] = opaque(src.get_pixel(sx as u32, sy as u32));
            }
        }
    }
    Grid { w, h, pixels }
}

/// The resampled grid of a loaded picture, kept per file, size and fit.
pub fn grid_for(key: &ImageKey, img: &RgbaImage, w: u16, h: u16, fit: Fit) -> Rc<Grid> {
    let k = (key.clone(), w, h, fit);
    if let Some(hit) = GRIDS.with(|g| g.borrow().get(&k).cloned()) {
        return hit;
    }
    let grid = Rc::new(make_grid(img, w, h, fit));
    GRIDS.with(|g| {
        let mut g = g.borrow_mut();
        if g.len() >= 256 {
            g.clear();
        }
        g.insert(k, grid.clone());
    });
    grid
}

// ---- Kitty graphics protocol ---------------------------------------------------------------------------------

/// The picture as PNG bytes, which is what the Kitty protocol is sent (`f=100`).
pub fn png_bytes(img: &RgbaImage) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = img.write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

/// The escape sequence that stores a picture in the terminal under `id`, without showing it (`a=t`), in chunks of at most
/// 4096 base64 characters. `q=2` stops the terminal from answering, so nothing lands on the input stream.
pub fn kitty_transmit(id: u32, png: &[u8]) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let mut out = String::new();
    let chunks: Vec<&str> = b64.as_bytes().chunks(4096).map(|c| std::str::from_utf8(c).unwrap_or("")).collect();
    let last = chunks.len().saturating_sub(1);
    for (i, chunk) in chunks.iter().enumerate() {
        let more = if i < last { 1 } else { 0 };
        if i == 0 {
            out.push_str(&format!("\x1b_Ga=t,f=100,i={id},q=2,m={more};{chunk}\x1b\\"));
        } else {
            out.push_str(&format!("\x1b_Gm={more},q=2;{chunk}\x1b\\"));
        }
    }
    out
}

/// Moves the cursor to cell (`x`, `y`) (0-based) and places the stored picture `id` over `cols` x `rows` cells (`a=p`).
/// `crop` is the part of the picture to show, in its own pixels (`x`, `y`, `w`, `h`); `None` shows all of it.
/// `C=1` keeps the cursor where it is.
pub fn kitty_place(id: u32, x: u16, y: u16, cols: u16, rows: u16, crop: Option<(u32, u32, u32, u32)>) -> String {
    let src = match crop {
        Some((sx, sy, sw, sh)) => format!(",x={sx},y={sy},w={sw},h={sh}"),
        None => String::new(),
    };
    format!("\x1b[{};{}H\x1b_Ga=p,i={id},c={cols},r={rows}{src},C=1,q=2\x1b\\", y as u32 + 1, x as u32 + 1)
}

/// The part of an `iw` x `ih` picture that fills `w` x `h` cells without stretching (a cell is taken as 1:2): the picture
/// is cut to the rectangle's aspect around its centre. Returns `(x, y, width, height)` in picture pixels.
pub fn cover_crop(iw: u32, ih: u32, w: u16, h: u16) -> (u32, u32, u32, u32) {
    let target = w as f64 / (2.0 * h as f64);
    if iw as f64 / ih as f64 > target {
        let cw = ((ih as f64 * target).round() as u32).clamp(1, iw);
        ((iw - cw) / 2, 0, cw, ih)
    } else {
        let ch = ((iw as f64 / target).round() as u32).clamp(1, ih);
        (0, (ih - ch) / 2, iw, ch)
    }
}

/// Removes every placement from the screen but keeps the stored pictures (`a=d,d=a`).
pub fn kitty_clear() -> &'static str {
    "\x1b_Ga=d,d=a,q=2\x1b\\"
}

/// One picture to place on the screen with the Kitty protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KittyPlacement {
    pub key: ImageKey,
    pub x: u16,
    pub y: u16,
    pub cols: u16,
    pub rows: u16,
    /// The part of the picture to show, in its own pixels; `None` is all of it.
    pub crop: Option<(u32, u32, u32, u32)>,
}

/// What the terminal has been sent: stored pictures by key, and the placements on screen now. `emit` returns the bytes to
/// write so the screen shows exactly `wanted`: nothing when it is unchanged (no flicker), otherwise clear, store what is new, place all.
#[derive(Default)]
pub struct KittyState {
    stored: HashMap<ImageKey, u32>,
    next_id: u32,
    shown: Vec<KittyPlacement>,
}

impl KittyState {
    pub fn new() -> KittyState {
        KittyState { stored: HashMap::new(), next_id: 1, shown: Vec::new() }
    }

    /// The escape sequences that bring the screen from the last `emit` to `wanted` (empty when nothing changes).
    /// `png_of` gives the PNG bytes of a picture that has not been stored yet.
    pub fn emit(&mut self, wanted: &[KittyPlacement], png_of: &dyn Fn(&ImageKey) -> Option<Vec<u8>>) -> String {
        if wanted == self.shown.as_slice() {
            return String::new();
        }
        let mut out = String::new();
        if !self.shown.is_empty() {
            out.push_str(kitty_clear());
        }
        for p in wanted {
            let id = match self.stored.get(&p.key) {
                Some(id) => *id,
                None => {
                    let Some(png) = png_of(&p.key) else { continue };
                    let id = self.next_id;
                    self.next_id += 1;
                    out.push_str(&kitty_transmit(id, &png));
                    self.stored.insert(p.key.clone(), id);
                    id
                }
            };
            out.push_str(&kitty_place(id, p.x, p.y, p.cols, p.rows, p.crop));
        }
        self.shown = wanted.to_vec();
        out
    }

    /// The sequence that removes everything this state has placed (used when the screen is left).
    pub fn finish(&mut self) -> String {
        let had = !self.shown.is_empty();
        self.shown.clear();
        if had { kitty_clear().to_string() } else { String::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn px(r: u8, g: u8, b: u8) -> Rgba<u8> {
        Rgba([r, g, b, 255])
    }
    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k: &str| m.get(k).cloned()
    }

    #[test]
    fn detection_reads_the_environment_only() {
        assert_eq!(detect_from(&env(&[("TERM", "xterm-256color")])), Protocol::HalfBlocks);
        assert_eq!(detect_from(&env(&[("TERM", "xterm-kitty")])), Protocol::Kitty);
        assert_eq!(detect_from(&env(&[("TERM", "xterm-ghostty")])), Protocol::Kitty);
        assert_eq!(detect_from(&env(&[("TERM_PROGRAM", "ghostty")])), Protocol::Kitty);
        assert_eq!(detect_from(&env(&[("GHOSTTY_RESOURCES_DIR", "/usr/share/ghostty")])), Protocol::Kitty);
        assert_eq!(detect_from(&env(&[("KITTY_WINDOW_ID", "3")])), Protocol::Kitty);
        assert_eq!(detect_from(&env(&[("TERM_PROGRAM", "WezTerm")])), Protocol::HalfBlocks);
        assert_eq!(detect_from(&env(&[("TERM", "foot")])), Protocol::HalfBlocks);
        assert_eq!(detect_from(&env(&[])), Protocol::HalfBlocks);
    }

    #[test]
    fn tmux_and_screen_get_half_blocks_and_the_override_wins() {
        assert_eq!(detect_from(&env(&[("TERM", "xterm-kitty"), ("TMUX", "/tmp/tmux-1/default,1,0")])), Protocol::HalfBlocks);
        assert_eq!(detect_from(&env(&[("TERM", "xterm-kitty"), ("STY", "123.pts")])), Protocol::HalfBlocks);
        assert_eq!(detect_from(&env(&[("TERM", "xterm-kitty"), ("MATCHBOX_TUI_IMAGES", "halfblocks")])), Protocol::HalfBlocks);
        assert_eq!(detect_from(&env(&[("TERM", "xterm-256color"), ("MATCHBOX_TUI_IMAGES", "kitty")])), Protocol::Kitty);
        assert_eq!(detect_from(&env(&[("TERM", "xterm-kitty"), ("MATCHBOX_TUI_IMAGES", "auto")])), Protocol::Kitty);
        assert_eq!(detect_from(&env(&[("TERM", "xterm-kitty"), ("MATCHBOX_TUI_IMAGES", "nonsense")])), Protocol::Kitty);
    }

    #[test]
    fn choices_parse_and_unimplemented_protocols_use_half_blocks() {
        assert_eq!(Choice::parse("AUTO"), Some(Choice::Auto));
        assert_eq!(Choice::parse("Kitty"), Some(Choice::Kitty));
        assert_eq!(Choice::parse("nope"), None);
        assert_eq!(Choice::Auto.resolve(Protocol::Kitty), Protocol::Kitty);
        assert_eq!(Choice::Sixel.resolve(Protocol::Kitty), Protocol::HalfBlocks);
        assert_eq!(Choice::Iterm2.resolve(Protocol::Kitty), Protocol::HalfBlocks);
        assert_eq!(Choice::Kitty.resolve(Protocol::HalfBlocks), Protocol::Kitty);
        assert_eq!(Protocol::Kitty.name(), "kitty");
        assert_eq!(Protocol::HalfBlocks.name(), "halfblocks");
    }

    #[test]
    fn a_picture_at_its_own_pixel_size_is_copied_pixel_for_pixel() {
        let mut img = RgbaImage::new(2, 4);
        img.put_pixel(0, 0, px(255, 0, 0));
        img.put_pixel(1, 0, px(0, 255, 0));
        img.put_pixel(0, 1, px(0, 0, 255));
        img.put_pixel(1, 3, px(9, 8, 7));
        let g = make_grid(&img, 2, 2, Fit::Contain);
        assert_eq!(g.pixel(0, 0), Some((255, 0, 0)));
        assert_eq!(g.pixel(1, 0), Some((0, 255, 0)));
        assert_eq!(g.pixel(0, 1), Some((0, 0, 255)));
        assert_eq!(g.pixel(1, 3), Some((9, 8, 7)));
        assert_eq!(g.pixel(1, 1), None, "a pixel nobody set is transparent");
        assert_eq!(g.pixel(5, 0), None);
    }

    #[test]
    fn transparent_pixels_stay_transparent() {
        let mut img = RgbaImage::new(1, 2);
        img.put_pixel(0, 0, Rgba([10, 20, 30, 255]));
        img.put_pixel(0, 1, Rgba([10, 20, 30, 40]));
        let g = make_grid(&img, 1, 1, Fit::Contain);
        assert_eq!(g.pixel(0, 0), Some((10, 20, 30)));
        assert_eq!(g.pixel(0, 1), None);
    }

    #[test]
    fn contain_keeps_the_aspect_and_centres_cover_fills_the_rectangle() {
        // A wide 4x1 picture in 4x4 cells (4 x 8 pixels): contain makes it 4 x 1 pixels in the middle rows.
        let mut wide = RgbaImage::new(4, 1);
        for x in 0..4 {
            wide.put_pixel(x, 0, px(200, 100, 50));
        }
        let contain = make_grid(&wide, 4, 4, Fit::Contain);
        let filled: Vec<u16> = (0..8).filter(|&y| contain.pixel(0, y).is_some()).collect();
        assert_eq!(filled.len(), 1, "one pixel row: {filled:?}");
        assert!(filled[0] == 3 || filled[0] == 4, "centred: {filled:?}");
        let cover = make_grid(&wide, 4, 4, Fit::Cover);
        assert!((0..8).all(|y| (0..4).all(|x| cover.pixel(x, y).is_some())), "cover fills every pixel");
    }

    #[test]
    fn the_cell_box_of_a_contained_picture_is_centred() {
        assert_eq!(contain_box(4, 4, 10, 5), Placement { dx: 0, dy: 0, cols: 10, rows: 5 });
        assert_eq!(contain_box(8, 4, 8, 8), Placement { dx: 0, dy: 3, cols: 8, rows: 2 });
        let tall = contain_box(1, 8, 8, 2);
        assert_eq!((tall.cols, tall.rows), (1, 2));
        assert_eq!(tall.dx, 3);
    }

    #[test]
    fn cover_cuts_the_picture_to_the_rectangle_around_its_centre() {
        // 100 x 50 picture into 4 x 4 cells (aspect 4:8 = 0.5): 25 wide, centred.
        assert_eq!(cover_crop(100, 50, 4, 4), (37, 0, 25, 50));
        // 50 x 100 picture into 8 x 2 cells (aspect 2): 50 wide, 25 tall, centred vertically.
        assert_eq!(cover_crop(50, 100, 8, 2), (0, 37, 50, 25));
        assert_eq!(cover_crop(8, 8, 4, 2), (0, 0, 8, 8));
    }

    #[test]
    fn kitty_sequences_follow_the_protocol() {
        assert_eq!(kitty_place(7, 4, 2, 10, 5, None), "\x1b[3;5H\x1b_Ga=p,i=7,c=10,r=5,C=1,q=2\x1b\\");
        assert_eq!(kitty_place(7, 0, 0, 4, 2, Some((1, 2, 30, 40))), "\x1b[1;1H\x1b_Ga=p,i=7,c=4,r=2,x=1,y=2,w=30,h=40,C=1,q=2\x1b\\");
        assert_eq!(kitty_clear(), "\x1b_Ga=d,d=a,q=2\x1b\\");
        let one = kitty_transmit(3, &[1, 2, 3]);
        assert_eq!(one, "\x1b_Ga=t,f=100,i=3,q=2,m=0;AQID\x1b\\");
        let big = kitty_transmit(1, &vec![0u8; 4000]);
        assert!(big.starts_with("\x1b_Ga=t,f=100,i=1,q=2,m=1;"));
        assert!(big.contains("\x1b_Gm=0,q=2;"));
        assert_eq!(big.matches("\x1b_G").count(), 2);
    }

    #[test]
    fn the_kitty_state_sends_a_picture_once_and_nothing_when_unchanged() {
        let key = ImageKey { path: "/a.png".into(), len: 1, modified_ns: 1 };
        let p = |x| KittyPlacement { key: key.clone(), x, y: 1, cols: 4, rows: 2, crop: None };
        let png = |_: &ImageKey| Some(vec![1u8, 2, 3]);
        let mut st = KittyState::new();
        let first = st.emit(&[p(0)], &png);
        assert!(first.contains("a=t,f=100,i=1") && first.contains("a=p,i=1,c=4,r=2"), "{first:?}");
        assert!(!first.contains("a=d"), "nothing to clear the first time");
        assert_eq!(st.emit(&[p(0)], &png), "", "unchanged: no output");
        let moved = st.emit(&[p(5)], &png);
        assert!(moved.starts_with("\x1b_Ga=d,d=a"), "{moved:?}");
        assert!(!moved.contains("a=t"), "already stored: placed without sending again");
        assert!(moved.contains("a=p,i=1") && moved.contains("\x1b[2;6H"));
        assert_eq!(st.emit(&[], &png), kitty_clear());
        assert_eq!(st.emit(&[], &png), "");
        assert_eq!(st.finish(), "");
    }

    #[test]
    fn a_missing_file_does_not_load() {
        assert!(load("/nonexistent/definitely-not-here.png").is_none());
        assert!(load("/tmp").is_none(), "a folder is not a picture");
    }
}
