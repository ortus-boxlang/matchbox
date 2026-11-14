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
//!
//! A picture can never take the program down. Every decode and every draw runs under `guard` (a caught panic is the same as a
//! bad file: nothing is drawn); a file is checked against size and pixel limits before it is decoded; a file that failed is
//! remembered (by path, size and modification time) so a screen drawing it every frame does not decode it again; and the
//! caches are bounded and evict the oldest entry. See `Limits` and `guard`.
use image::imageops::{self, FilterType};
use image::RgbaImage;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::panic::{self, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::Once;

/// Largest picture file that is decoded, in bytes (20 MB). Override with `MATCHBOX_TUI_IMAGE_MAX_BYTES`.
pub const DEFAULT_MAX_BYTES: u64 = 20 * 1024 * 1024;
/// Largest picture that is decoded, in pixels (12 megapixels, read from the file header before decoding). Override with `MATCHBOX_TUI_IMAGE_MAX_PIXELS`.
/// Measured on a release build, a 12 megapixel PNG takes about a third of a second to decode and reduce; a 20 megapixel one takes about 0.6 s.
pub const DEFAULT_MAX_PIXELS: u64 = 12_000_000;
/// A decoded picture bigger than this on its longest side is scaled down to it before it is kept (a terminal never needs more).
pub const MAX_STORE_DIM: u32 = 2048;
/// Decoded pictures kept at most, and the most memory they may use.
pub const MAX_CACHE_ENTRIES: usize = 48;
pub const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
/// Resampled grids kept at most, and the most memory they may use.
pub const MAX_GRID_ENTRIES: usize = 256;
pub const MAX_GRID_BYTES: usize = 32 * 1024 * 1024;
/// Files remembered as failed (past this the memory is cleared and files are tried again).
pub const MAX_FAILED: usize = 512;
/// The most cells wide or high a picture rectangle may be; a bigger one is refused (no terminal is that large).
pub const MAX_CELLS: u16 = 1024;

/// The limits a picture is checked against before it is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// File size in bytes.
    pub max_bytes: u64,
    /// Width times height in pixels.
    pub max_pixels: u64,
}

/// The limits from environment variables: `MATCHBOX_TUI_IMAGE_MAX_BYTES` and `MATCHBOX_TUI_IMAGE_MAX_PIXELS` (positive whole
/// numbers; anything else keeps the default).
pub fn limits_from(get: &dyn Fn(&str) -> Option<String>) -> Limits {
    let number = |name: &str, default: u64| get(name).and_then(|v| v.trim().parse::<u64>().ok()).filter(|n| *n > 0).unwrap_or(default);
    Limits { max_bytes: number("MATCHBOX_TUI_IMAGE_MAX_BYTES", DEFAULT_MAX_BYTES), max_pixels: number("MATCHBOX_TUI_IMAGE_MAX_PIXELS", DEFAULT_MAX_PIXELS) }
}

/// `limits_from` on the real environment.
pub fn limits() -> Limits {
    limits_from(&|name| std::env::var(name).ok())
}

thread_local! {
    /// How many panics are being caught on this thread right now (the panic hook stays quiet while it is above 0).
    static QUIET: Cell<u32> = const { Cell::new(0) };
    /// A fault to trigger once on this thread (set by tests): "decode", "draw" or "place".
    static FAULT: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// Whether the fault named in `MATCHBOX_TUI_IMAGE_FAULT` has already been triggered on this thread.
    static FAULT_ENV_FIRED: Cell<bool> = const { Cell::new(false) };
    /// How many pictures were really decoded on this thread (read by tests).
    static DECODES: Cell<u64> = const { Cell::new(0) };
}
static HOOK: Once = Once::new();
/// Panics the hook let through because no guard was active (read by tests to show a guarded panic prints nothing).
#[cfg(test)]
pub(crate) static LOUD_PANICS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Whether a caught panic can really be recovered from in this build. The release profile of the `matchbox` binary and of the native
/// runner stub uses `panic = "unwind"`, so it is true there; a build with `panic = "abort"` ends the process on the first panic
/// whatever `guard` does, so there the protection against a decoder panic is the limits and the checks made before decoding.
pub const PANICS_ARE_CAUGHT: bool = cfg!(panic = "unwind");

/// Runs `f`; a panic inside it is caught and turned into `None`, and the panic hook prints nothing for it (a message in the
/// middle of a full-screen terminal would wreck the display, and the program must go on drawing the next frame). Everything
/// that decodes or draws a picture goes through this.
pub fn guard<T>(f: impl FnOnce() -> T) -> Option<T> {
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if QUIET.with(|q| q.get()) == 0 {
                #[cfg(test)]
                LOUD_PANICS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                previous(info);
            }
        }));
    });
    QUIET.with(|q| q.set(q.get() + 1));
    let result = panic::catch_unwind(AssertUnwindSafe(f));
    QUIET.with(|q| q.set(q.get() - 1));
    result.ok()
}

/// Makes the next `fault( kind )` on this thread panic (`"decode"`, `"draw"` or `"place"`). For tests.
#[doc(hidden)]
pub fn inject_fault(kind: &'static str) {
    FAULT.with(|f| f.set(Some(kind)));
}

/// Panics once if a fault of this `kind` was injected with `inject_fault`, or named in `MATCHBOX_TUI_IMAGE_FAULT` (used by the
/// end-to-end tests to prove a decoder panic does not take the program down; it does nothing when the variable is not set).
fn fault(kind: &str) {
    if FAULT.with(|f| f.get()) == Some(kind) {
        FAULT.with(|f| f.set(None));
        panic!("injected {kind} fault");
    }
    if std::env::var("MATCHBOX_TUI_IMAGE_FAULT").map(|v| v == kind).unwrap_or(false) && !FAULT_ENV_FIRED.with(|f| f.replace(true)) {
        panic!("injected {kind} fault");
    }
}

/// The `fault` check for code outside this module (the canvas calls it where a placement is worked out).
pub fn fault_point(kind: &str) {
    fault(kind);
}

/// How many pictures have really been decoded on this thread (not counting cache hits or refusals before decoding).
#[doc(hidden)]
pub fn decode_attempts() -> u64 {
    DECODES.with(|d| d.get())
}

/// A cache that keeps at most `max_entries` entries and `max_bytes` of data and drops the oldest first.
pub struct ByteCache<K: Hash + Eq + Clone, V: Clone> {
    map: HashMap<K, (V, usize)>,
    order: VecDeque<K>,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
}

impl<K: Hash + Eq + Clone, V: Clone> ByteCache<K, V> {
    pub fn new(max_entries: usize, max_bytes: usize) -> Self {
        ByteCache { map: HashMap::new(), order: VecDeque::new(), bytes: 0, max_entries, max_bytes }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        self.map.get(key).map(|(v, _)| v.clone())
    }

    /// Keeps `value` (`size` bytes), dropping the oldest entries until it fits. A value bigger than the whole cache is not kept.
    pub fn insert(&mut self, key: K, value: V, size: usize) {
        if size > self.max_bytes {
            return;
        }
        if let Some((_, old)) = self.map.remove(&key) {
            self.bytes -= old;
            self.order.retain(|k| k != &key);
        }
        while !self.order.is_empty() && (self.map.len() >= self.max_entries || self.bytes + size > self.max_bytes) {
            if let Some(oldest) = self.order.pop_front() {
                if let Some((_, old)) = self.map.remove(&oldest) {
                    self.bytes -= old;
                }
            }
        }
        self.bytes += size;
        self.order.push_back(key.clone());
        self.map.insert(key, (value, size));
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }
}

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
    static PICTURES: RefCell<ByteCache<ImageKey, Rc<RgbaImage>>> = RefCell::new(ByteCache::new(MAX_CACHE_ENTRIES, MAX_CACHE_BYTES));
    static GRIDS: RefCell<ByteCache<(ImageKey, u16, u16, Fit), Rc<Grid>>> = RefCell::new(ByteCache::new(MAX_GRID_ENTRIES, MAX_GRID_BYTES));
    /// Files that failed to load, by path, size and modification time: they are not tried again until they change.
    static FAILED: RefCell<HashSet<ImageKey>> = RefCell::new(HashSet::new());
}

/// Decodes the file under the limits, or `None`: the header is read first and a picture with more pixels than `limits.max_pixels`
/// is refused before any pixel is decoded; the decoder is also given an allocation limit. A picture whose longest side is over
/// `MAX_STORE_DIM` is scaled down to it.
fn decode(path: &str, limits: Limits) -> Option<RgbaImage> {
    fault("decode");
    let open = || image::ImageReader::open(path).ok()?.with_guessed_format().ok();
    let (w, h) = open()?.into_dimensions().ok()?;
    if w == 0 || h == 0 || (w as u64) * (h as u64) > limits.max_pixels {
        return None;
    }
    let mut reader = open()?;
    let mut decode_limits = image::Limits::default();
    decode_limits.max_image_width = Some(w);
    decode_limits.max_image_height = Some(h);
    decode_limits.max_alloc = Some(limits.max_pixels.saturating_mul(8).min(1 << 31));
    reader.limits(decode_limits);
    let decoded = reader.decode().ok()?;
    let longest = decoded.width().max(decoded.height());
    if longest > MAX_STORE_DIM {
        // A big picture is reduced with the box filter of `thumbnail` (fast for large reductions) before it is converted and kept.
        let scale = MAX_STORE_DIM as f64 / longest as f64;
        let nw = ((decoded.width() as f64 * scale).round() as u32).max(1);
        let nh = ((decoded.height() as f64 * scale).round() as u32).max(1);
        return Some(decoded.thumbnail_exact(nw, nh).to_rgba8());
    }
    Some(decoded.to_rgba8())
}

/// A picture file decoded (PNG, JPEG, GIF or WebP; the first frame of a GIF), or `None` when it is missing, is not a picture,
/// is over the limits (see `Limits`), is damaged, or the decoder panicked. It never panics and never decodes more than the
/// limits allow. Decoded pictures are kept (bounded, oldest dropped first), so drawing the same file every frame costs a lookup;
/// a file that failed is remembered by path, size and modification time and is not tried again until it changes.
pub fn load(path: &str) -> Option<(ImageKey, Rc<RgbaImage>)> {
    guard(|| load_unguarded(path)).flatten()
}

fn load_unguarded(path: &str) -> Option<(ImageKey, Rc<RgbaImage>)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let modified_ns = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    let key = ImageKey { path: path.to_string(), len: meta.len(), modified_ns };
    if let Some(hit) = PICTURES.with(|p| p.borrow().get(&key)) {
        return Some((key, hit));
    }
    if FAILED.with(|f| f.borrow().contains(&key)) {
        return None;
    }
    let limits = limits();
    let decoded = if meta.len() == 0 || meta.len() > limits.max_bytes {
        None
    } else {
        DECODES.with(|d| d.set(d.get() + 1));
        guard(|| decode(path, limits)).flatten()
    };
    match decoded {
        Some(img) => {
            let rc = Rc::new(img);
            let size = rc.width() as usize * rc.height() as usize * 4;
            PICTURES.with(|p| p.borrow_mut().insert(key.clone(), rc.clone(), size));
            Some((key, rc))
        }
        None => {
            FAILED.with(|f| {
                let mut f = f.borrow_mut();
                if f.len() >= MAX_FAILED {
                    f.clear();
                }
                f.insert(key);
            });
            None
        }
    }
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
    fault("draw");
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

/// The resampled grid of a loaded picture, kept per file, size and fit (bounded, oldest dropped first). `None` when the rectangle is
/// larger than `MAX_CELLS` or the resampling panicked.
pub fn grid_for(key: &ImageKey, img: &RgbaImage, w: u16, h: u16, fit: Fit) -> Option<Rc<Grid>> {
    if w > MAX_CELLS || h > MAX_CELLS {
        return None;
    }
    let k = (key.clone(), w, h, fit);
    if let Some(hit) = GRIDS.with(|g| g.borrow().get(&k)) {
        return Some(hit);
    }
    let grid = Rc::new(guard(|| make_grid(img, w, h, fit))?);
    GRIDS.with(|g| g.borrow_mut().insert(k, grid.clone(), w as usize * 2 * h as usize * 4 + 64));
    Some(grid)
}

/// The picture as PNG bytes, which is what the Kitty protocol is sent (`f=100`). Empty when encoding panicked.
pub fn png_bytes(img: &RgbaImage) -> Vec<u8> {
    guard(|| {
        let mut out = std::io::Cursor::new(Vec::new());
        let _ = img.write_to(&mut out, image::ImageFormat::Png);
        out.into_inner()
    })
    .unwrap_or_default()
}

// ---- Kitty graphics protocol ---------------------------------------------------------------------------------

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

    // ---- hardening: a bad picture never takes the program down ----
    use std::sync::atomic::Ordering;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("matchbox-tui-img-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn tiny_png() -> Vec<u8> {
        let mut img = RgbaImage::new(16, 16);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgba([(x * 16) as u8, (y * 16) as u8, 128, 255]);
        }
        png_bytes(&img)
    }
    fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> String {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path.to_string_lossy().into_owned()
    }
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xffff_ffffu32;
        for b in data {
            crc ^= *b as u32;
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    /// A PNG that claims to be `w` x `h` pixels and has no pixel data at all: a decompression bomb header.
    fn bomb_png(w: u32, h: u32) -> Vec<u8> {
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
        for (kind, data) in [(&b"IHDR"[..], ihdr), (&b"IEND"[..], Vec::new())] {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(&data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc32(&body).to_be_bytes());
        }
        out
    }

    #[test]
    fn panics_are_only_caught_in_builds_that_unwind() {
        // Tests run with the unwind strategy; a build with panic = "abort" cannot catch (see PANICS_ARE_CAUGHT).
        assert!(PANICS_ARE_CAUGHT);
        assert_eq!(guard(|| 7), Some(7));
        assert_eq!(guard(|| -> u32 { panic!("caught") }), None);
    }

    #[test]
    fn limits_come_from_the_environment_and_ignore_nonsense() {
        let none = limits_from(&env(&[]));
        assert_eq!(none, Limits { max_bytes: DEFAULT_MAX_BYTES, max_pixels: DEFAULT_MAX_PIXELS });
        let set = limits_from(&env(&[("MATCHBOX_TUI_IMAGE_MAX_BYTES", "1000"), ("MATCHBOX_TUI_IMAGE_MAX_PIXELS", " 5000 ")]));
        assert_eq!(set, Limits { max_bytes: 1000, max_pixels: 5000 });
        let bad = limits_from(&env(&[("MATCHBOX_TUI_IMAGE_MAX_BYTES", "lots"), ("MATCHBOX_TUI_IMAGE_MAX_PIXELS", "0")]));
        assert_eq!(bad, none);
    }

    #[test]
    fn the_cache_drops_the_oldest_by_count_and_by_bytes_and_never_keeps_what_cannot_fit() {
        let mut c: ByteCache<u32, u32> = ByteCache::new(3, 100);
        for i in 1..=4 {
            c.insert(i, i * 10, 10);
        }
        assert_eq!((c.get(&1), c.get(&2), c.get(&4)), (None, Some(20), Some(40)));
        assert_eq!(c.len(), 3);
        c.insert(5, 50, 85);
        assert!(c.bytes() <= 100, "{}", c.bytes());
        assert_eq!(c.get(&5), Some(50));
        assert_eq!(c.get(&2), None);
        c.insert(6, 60, 101);
        assert_eq!(c.get(&6), None, "bigger than the whole cache");
        c.insert(5, 55, 10);
        assert_eq!((c.get(&5), c.len(), c.bytes()), (Some(55), 2, 20), "re-inserting replaces and recounts");
    }

    #[test]
    fn damaged_files_return_none_and_never_panic() {
        let dir = temp_dir("damaged");
        let good = tiny_png();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("truncated.png", good[..good.len() / 2].to_vec()),
            ("garbage-body.png", [&good[..33], &[0xde, 0xad, 0xbe, 0xef, 0x13, 0x37][..], &vec![0x55u8; 300][..]].concat()),
            ("empty.png", Vec::new()),
            ("notes.png", b"this is a text file, not a picture\n".to_vec()),
            ("signature-only.png", vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
            ("webp-lie.webp", b"RIFF\x10\x00\x00\x00WEBPVP8 junkjunk".to_vec()),
            ("gif-lie.gif", b"GIF89a\x01\x00\x01\x00junk".to_vec()),
        ];
        for (name, bytes) in cases {
            let path = write(&dir, name, &bytes);
            assert!(load(&path).is_none(), "{name} must not load");
        }
        // a real JPEG with its middle destroyed, and one cut short
        let mut img = RgbaImage::new(32, 32);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgba([(x * 8) as u8, (y * 8) as u8, 200, 255]);
        }
        let jpg = dir.join("ok.jpg");
        image::DynamicImage::ImageRgba8(img).to_rgb8().save(&jpg).unwrap();
        let bytes = std::fs::read(&jpg).unwrap();
        assert!(load(jpg.to_str().unwrap()).is_some(), "the undamaged JPEG loads");
        let mut corrupt = bytes.clone();
        for b in corrupt[bytes.len() / 3..2 * bytes.len() / 3].iter_mut() {
            *b = 0x5a;
        }
        let corrupt_path = write(&dir, "corrupt.jpg", &corrupt);
        let _ = load(&corrupt_path); // may fail or decode with damage; it must not panic or hang
        let short_path = write(&dir, "short.jpg", &bytes[..bytes.len() / 2]);
        let _ = load(&short_path);
        // a directory and a missing path
        assert!(load(dir.to_str().unwrap()).is_none());
        assert!(load(dir.join("nope.png").to_str().unwrap()).is_none());
        assert!(load("").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picture_that_claims_too_many_pixels_is_refused_from_its_header() {
        let dir = temp_dir("bomb");
        let before = decode_attempts();
        // 100000 x 100000 = ten billion pixels, in a 70 byte file
        let path = write(&dir, "bomb.png", &bomb_png(100_000, 100_000));
        let t = std::time::Instant::now();
        assert!(load(&path).is_none());
        assert!(t.elapsed() < std::time::Duration::from_millis(500), "refused without decoding: {:?}", t.elapsed());
        assert_eq!(decode_attempts() - before, 1);
        // just over the default pixel limit of 12 megapixels
        let over = write(&dir, "over.png", &bomb_png(4000, 3001));
        assert!(load(&over).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_over_the_size_limit_is_refused_without_decoding() {
        let dir = temp_dir("big-file");
        let mut bytes = tiny_png();
        bytes.resize(DEFAULT_MAX_BYTES as usize + 1, 0);
        let path = write(&dir, "big.png", &bytes);
        let before = decode_attempts();
        assert!(load(&path).is_none());
        assert_eq!(decode_attempts(), before, "refused on its size alone");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failure_is_remembered_until_the_file_changes() {
        let dir = temp_dir("negative");
        let path = write(&dir, "later.png", b"not a picture yet");
        let start = decode_attempts();
        for _ in 0..20 {
            assert!(load(&path).is_none());
        }
        assert_eq!(decode_attempts() - start, 1, "twenty frames, one decode attempt");
        // the same size but a new modification time is tried again
        let f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
        drop(f);
        assert!(load(&path).is_none());
        assert_eq!(decode_attempts() - start, 2, "a touched file is retried");
        // the real picture replaces it: loaded now, and from the cache afterwards
        std::fs::write(&path, tiny_png()).unwrap();
        assert!(load(&path).is_some());
        assert!(load(&path).is_some());
        assert_eq!(decode_attempts() - start, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_panic_in_the_decoder_is_caught_is_silent_and_the_next_picture_loads() {
        let dir = temp_dir("fault-decode");
        let first = write(&dir, "first.png", &tiny_png());
        let second = write(&dir, "second.png", &[tiny_png(), vec![0u8; 3]].concat());
        guard(|| ()); // make sure the quiet hook is installed
        let loud = LOUD_PANICS.load(Ordering::SeqCst);
        inject_fault("decode");
        assert!(load(&first).is_none(), "a decoder panic is a bad file");
        assert_eq!(LOUD_PANICS.load(Ordering::SeqCst), loud, "the hook printed nothing");
        assert!(load(&first).is_none(), "and it is remembered, so it does not panic again every frame");
        assert!(load(&second).is_some(), "the next picture still loads");
        // a panic outside a guard is still reported (so the hook is doing its job)
        let _ = std::panic::catch_unwind(|| panic!("not guarded"));
        assert!(LOUD_PANICS.load(Ordering::SeqCst) > loud);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resampling_that_panics_leaves_no_grid_and_the_next_call_works() {
        let mut img = RgbaImage::new(4, 4);
        for p in img.pixels_mut() {
            *p = Rgba([1, 2, 3, 255]);
        }
        let key = ImageKey { path: "/fault-grid.png".into(), len: 1, modified_ns: 1 };
        inject_fault("draw");
        assert!(grid_for(&key, &img, 2, 2, Fit::Contain).is_none());
        assert!(grid_for(&key, &img, 2, 2, Fit::Contain).is_some());
        assert!(grid_for(&key, &img, MAX_CELLS + 1, 2, Fit::Contain).is_none(), "an absurd rectangle is refused");
    }

    #[test]
    fn a_large_picture_within_the_limits_decodes_quickly_and_is_kept_small() {
        // 4000 x 3000 = 12 megapixels, exactly the default limit; smooth colour so the file stays small and the decode is not cheap
        let dir = temp_dir("large");
        let mut img = RgbaImage::new(4000, 3000);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgba([(x / 16) as u8, (y / 12) as u8, 90, 255]);
        }
        let path = dir.join("large.png");
        img.save(&path).unwrap();
        let path = path.to_string_lossy().into_owned();
        let t = std::time::Instant::now();
        let (_, loaded) = load(&path).expect("within the limits");
        let took = t.elapsed();
        eprintln!("12 megapixel decode and scale: {took:?}");
        assert!(loaded.width().max(loaded.height()) <= MAX_STORE_DIM, "kept at most {MAX_STORE_DIM} pixels on a side");
        // Release builds: well under a second (about 0.35 s measured). Debug builds compile the decoders without optimisation and are
        // several times slower, so there the bound only guards against a hang.
        assert!(took < std::time::Duration::from_secs(if cfg!(debug_assertions) { 20 } else { 1 }), "{took:?}");
        let t = std::time::Instant::now();
        assert!(load(&path).is_some());
        assert!(t.elapsed() < std::time::Duration::from_millis(50), "the second time is a cache lookup");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
