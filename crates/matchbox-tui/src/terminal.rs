use std::cell::RefCell;
use std::io;

use crossterm::event::DisableMouseCapture as DisableMouseCaptureEvent;
use crossterm::event::EnableMouseCapture as EnableMouseCaptureEvent;
use crossterm::event::{
    self, Event as CrossTermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use matchbox_vm::bx_methods;
use matchbox_vm::types::{BxNativeObject, BxVM, BxValue, Tracer};

use crate::canvas::{self, BorderStyle, CellStyle, DrawCmd};
use crate::color;
use crate::imaging::{self, Choice, Fit, KittyPlacement, KittyState};
use crate::widget::{WidgetKind, WidgetRegistry};

pub struct TUI {
    terminal: Option<Terminal<CrosstermBackend<io::Stderr>>>,
    frame_widgets: Vec<(usize, u16, u16, u16, u16, i32)>,
    /// Immediate-mode drawing for the frame in progress (see `canvas`), painted after the widgets.
    canvas: Vec<DrawCmd>,
    mouse_enabled: bool,
    dirty: bool,
    /// What has been sent to the terminal for pictures drawn with the Kitty protocol.
    kitty: KittyState,
}

thread_local! {
    static CURRENT_TUI: RefCell<TUI> = RefCell::new(TUI::new());
}

impl std::fmt::Debug for TUI {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TUI")
            .field("terminal", &self.terminal.is_some())
            .field("frame_widgets", &self.frame_widgets.len())
            .field("mouse_enabled", &self.mouse_enabled)
            .field("dirty", &self.dirty)
            .finish()
    }
}

#[bx_methods]
impl TUI {
    pub fn bx_init(&mut self, vm: &mut dyn BxVM) -> Result<(), String> {
        vm.suspend_gc();
        if let Err(err) = self.init() {
            vm.resume_gc();
            return Err(err);
        }
        Ok(())
    }

    pub fn bx_begin_frame(&mut self) {
        self.begin_frame();
    }

    pub fn bx_end_frame(&mut self, vm: &mut dyn BxVM) -> Result<(), String> {
        self.end_frame(vm)
    }

    pub fn bx_print(
        &mut self,
        _vm: &mut dyn BxVM,
        x: f64,
        y: f64,
        text: String,
        color: String,
        bold: bool,
    ) -> Result<(), String> {
        self.print(x as u16, y as u16, &text, &color, bold)
    }

    /// `fill( x, y, width, height, style )`: paints a rectangle with the style's `bg`.
    pub fn bx_fill(&mut self, vm: &mut dyn BxVM, x: f64, y: f64, w: f64, h: f64, style: BxValue) -> Result<(), String> {
        let style = style_of(vm, style)?;
        self.canvas.push(DrawCmd::Fill { x: x as u16, y: y as u16, w: w as u16, h: h as u16, style });
        Ok(())
    }

    /// `text( x, y, text, style )`: `style` is a struct with any of `fg`, `bg`, `bold`, `dim`, `italic`, `underline` (or `{}`).
    pub fn bx_text(&mut self, vm: &mut dyn BxVM, x: f64, y: f64, text: String, style: BxValue) -> Result<(), String> {
        let style = style_of(vm, style)?;
        self.canvas.push(DrawCmd::Text { x: x as u16, y: y as u16, text, style });
        Ok(())
    }

    /// `gradient( x, y, text, from, to, style )`: text whose colour blends from `from` to `to` across its width.
    pub fn bx_gradient(&mut self, vm: &mut dyn BxVM, x: f64, y: f64, text: String, from: String, to: String, style: BxValue) -> Result<(), String> {
        let style = style_of(vm, style)?;
        let from = color::parse(&from).ok_or_else(|| format!("Unknown colour: {from}"))?;
        let to = color::parse(&to).ok_or_else(|| format!("Unknown colour: {to}"))?;
        self.canvas.push(DrawCmd::Gradient { x: x as u16, y: y as u16, text, from, to, style });
        Ok(())
    }

    /// `border( x, y, width, height, options )`: a bordered box. `options` has the style fields plus `border`
    /// (`rounded`, `plain`, `double`, `thick`), `title` and `titleFg`. The inside is filled when `bg` is set.
    pub fn bx_border(&mut self, vm: &mut dyn BxVM, x: f64, y: f64, w: f64, h: f64, options: BxValue) -> Result<(), String> {
        let style = style_of(vm, options)?;
        let (mut border, mut title, mut title_style) = (BorderStyle::Rounded, String::new(), CellStyle::default());
        if let Some(id) = options.as_gc_id() {
            if vm.struct_key_exists(id, "border") {
                border = BorderStyle::parse(&vm.to_string(vm.struct_get(id, "border")));
            }
            if vm.struct_key_exists(id, "title") {
                title = vm.to_string(vm.struct_get(id, "title"));
            }
            if vm.struct_key_exists(id, "titleFg") {
                let name = vm.to_string(vm.struct_get(id, "titleFg"));
                title_style.fg = Some(color::parse(&name).ok_or_else(|| format!("Unknown colour: {name}"))?);
                title_style.bold = true;
            }
        }
        self.canvas.push(DrawCmd::Border { x: x as u16, y: y as u16, w: w as u16, h: h as u16, border, style, title, title_style });
        Ok(())
    }

    /// `image_support()`: how a picture would be drawn with `protocol: "auto"` in this terminal, `"kitty"` or `"halfblocks"`,
    /// read from the environment only (the terminal is never asked). `MATCHBOX_TUI_IMAGES` overrides it.
    pub fn bx_image_support(&self) -> String {
        imaging::detect().name().to_string()
    }

    /// `render_to_string( width, height )`: the current frame's canvas drawn into memory as plain text, one line per
    /// row. Needs no terminal, so a UI can be tested.
    pub fn bx_render_to_string(&mut self, vm: &mut dyn BxVM, width: f64, height: f64) -> Result<BxValue, String> {
        let mut buf = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, width as u16, height as u16));
        canvas::render(&self.canvas, &mut buf);
        let id = vm.string_new(canvas::rows_text(&buf).join("\n"));
        Ok(BxValue::new_ptr(id))
    }

    pub fn bx_render_widget(
        &mut self,
        _vm: &mut dyn BxVM,
        widget_id: f64,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) {
        self.render_widget(
            widget_id as usize,
            x as u16,
            y as u16,
            width as u16,
            height as u16,
            0, // Default Z-index
        );
    }

    pub fn bx_render_widget_z(
        &mut self,
        _vm: &mut dyn BxVM,
        widget_id: f64,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        z_index: f64,
    ) {
        self.render_widget(
            widget_id as usize,
            x as u16,
            y as u16,
            width as u16,
            height as u16,
            z_index as i32,
        );
    }

    /// `poll_event( timeoutMs )`: the next event as a struct, or null when none arrives in time.
    /// `{type:"key", key, ctrl, alt, shift}` (key names: the character, or Enter, Tab, BackTab, Backspace, Escape, Left,
    /// Right, Up, Down, Home, End, PageUp, PageDown, Delete, Insert, F1..; with Ctrl+ or Alt+ in front),
    /// `{type:"click", x, y, widgetId?}`, `{type:"scroll", direction:"up"|"down", x, y}`, `{type:"resize", width, height}`.
    pub fn bx_poll_event(&self, vm: &mut dyn BxVM, timeout: f64) -> Result<BxValue, String> {
        if !event::poll(std::time::Duration::from_millis(timeout as u64)).map_err(|e| e.to_string())? {
            return Ok(BxValue::new_null());
        }
        let set_str = |vm: &mut dyn BxVM, s: usize, key: &str, value: &str| {
            let id = vm.string_new(value.to_string());
            vm.struct_set(s, key, BxValue::new_ptr(id));
        };
        match event::read().map_err(|e| e.to_string())? {
            CrossTermEvent::Key(key) if key.kind == KeyEventKind::Press => {
                let s = vm.struct_new();
                set_str(vm, s, "type", "key");
                set_str(vm, s, "key", &format_key(key));
                vm.struct_set(s, "ctrl", BxValue::new_bool(key.modifiers.contains(KeyModifiers::CONTROL)));
                vm.struct_set(s, "alt", BxValue::new_bool(key.modifiers.contains(KeyModifiers::ALT)));
                vm.struct_set(s, "shift", BxValue::new_bool(key.modifiers.contains(KeyModifiers::SHIFT)));
                Ok(BxValue::new_ptr(s))
            }
            CrossTermEvent::Mouse(mouse) => match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    let s = vm.struct_new();
                    set_str(vm, s, "type", "click");
                    vm.struct_set(s, "x", BxValue::new_number(mouse.column as f64));
                    vm.struct_set(s, "y", BxValue::new_number(mouse.row as f64));
                    if let Some(widget_id) = self.hit_test(mouse.column, mouse.row) {
                        vm.struct_set(s, "widgetId", BxValue::new_number(widget_id as f64));
                    }
                    Ok(BxValue::new_ptr(s))
                }
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    let s = vm.struct_new();
                    set_str(vm, s, "type", "scroll");
                    set_str(vm, s, "direction", if mouse.kind == MouseEventKind::ScrollUp { "up" } else { "down" });
                    vm.struct_set(s, "x", BxValue::new_number(mouse.column as f64));
                    vm.struct_set(s, "y", BxValue::new_number(mouse.row as f64));
                    Ok(BxValue::new_ptr(s))
                }
                _ => Ok(BxValue::new_null()),
            },
            CrossTermEvent::Resize(width, height) => {
                let s = vm.struct_new();
                set_str(vm, s, "type", "resize");
                vm.struct_set(s, "width", BxValue::new_number(width as f64));
                vm.struct_set(s, "height", BxValue::new_number(height as f64));
                Ok(BxValue::new_ptr(s))
            }
            _ => Ok(BxValue::new_null()),
        }
    }

    pub fn bx_size(&self, vm: &mut dyn BxVM) -> Result<BxValue, String> {
        let (w, h) = self.size()?;
        let s = vm.struct_new();
        vm.struct_set(s, "width", BxValue::new_number(w as f64));
        vm.struct_set(s, "height", BxValue::new_number(h as f64));
        Ok(BxValue::new_ptr(s))
    }

    pub fn bx_is_dirty(&self) -> bool {
        self.is_dirty()
    }

    pub fn bx_set_dirty(&mut self) {
        self.set_dirty();
    }

    pub fn bx_clear_widgets(&mut self) {
        WidgetRegistry::clear();
    }

    pub fn bx_clear(&mut self) -> Result<(), String> {
        self.clear()
    }

    pub fn bx_set_mouse(&mut self, enabled: bool) -> Result<(), String> {
        self.set_mouse(enabled)
    }

    pub fn bx_shutdown(&mut self, vm: &mut dyn BxVM) -> Result<(), String> {
        let result = self.shutdown();
        vm.resume_gc();
        result
    }
}

impl BxNativeObject for TUI {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        if name.eq_ignore_ascii_case("image") {
            return self.call_image(vm, args);
        }
        self.dispatch_method(vm, id, name, args)
    }

    fn trace(&self, tracer: &mut dyn Tracer) {
        WidgetRegistry::trace(tracer);
    }
}

impl TUI {
    pub fn new() -> Self {
        Self {
            terminal: None,
            frame_widgets: Vec::new(),
            canvas: Vec::new(),
            mouse_enabled: false,
            dirty: true,
            kitty: KittyState::new(),
        }
    }

    /// `image( x, y, width, height, path [, options] )`: draws a PNG, JPEG, GIF (first frame) or WebP file inside the rectangle of
    /// cells and returns `true`, or draws nothing and returns `false` when the file is missing or not a picture.
    /// `options` (a struct) may have `fit` (`"contain"`, the default, or `"cover"`) and `protocol` (`"auto"`, the default,
    /// `"kitty"`, `"halfblocks"`; `"sixel"` and `"iterm2"` are accepted and draw as half blocks for now).
    fn call_image(&mut self, vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
        if args.len() != 5 && args.len() != 6 {
            return Err(format!("image requires 5 or 6 arguments (x, y, width, height, path [, options]), got {}", args.len()));
        }
        let path = vm.to_string(args[4]);
        let (mut fit, mut choice) = (Fit::Contain, Choice::Auto);
        if let Some(opts) = args.get(5).and_then(|v| v.as_gc_id()) {
            if vm.struct_key_exists(opts, "fit") {
                fit = Fit::parse(&vm.to_string(vm.struct_get(opts, "fit")));
            }
            if vm.struct_key_exists(opts, "protocol") {
                let name = vm.to_string(vm.struct_get(opts, "protocol"));
                choice = Choice::parse(&name).ok_or_else(|| format!("Unknown image protocol: {name}"))?;
            }
        }
        let (x, y, w, h) = (args[0].as_number() as u16, args[1].as_number() as u16, args[2].as_number() as u16, args[3].as_number() as u16);
        // Never panics and never blocks beyond the limits: a rectangle that is empty or absurdly large, a file that is missing,
        // damaged, over the limits or that made the decoder panic all give false and draw nothing.
        if w == 0 || h == 0 || w > imaging::MAX_CELLS || h > imaging::MAX_CELLS || imaging::load(&path).is_none() {
            return Ok(BxValue::new_bool(false));
        }
        let protocol = choice.resolve(imaging::detect());
        self.canvas.push(DrawCmd::Image { x, y, w, h, path, fit, protocol });
        Ok(BxValue::new_bool(true))
    }

    pub fn with_current<F, R>(f: F) -> R
    where
        F: FnOnce(&mut TUI) -> R,
    {
        CURRENT_TUI.with(|tui| {
            let mut borrow = tui.borrow_mut();
            f(&mut borrow)
        })
    }

    pub fn set_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn set_dirty_val(&mut self, val: bool) {
        self.dirty = val;
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn init(&mut self) -> Result<(), String> {
        terminal::enable_raw_mode().map_err(|e| e.to_string())?;
        let mut stderr = io::stderr();
        execute!(stderr, EnterAlternateScreen, EnableMouseCaptureEvent)
            .map_err(|e| e.to_string())?;
        let backend = CrosstermBackend::new(stderr);
        let terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
        self.terminal = Some(terminal);
        self.mouse_enabled = true;
        Ok(())
    }

    pub fn begin_frame(&mut self) {
        self.frame_widgets.clear();
        self.canvas.clear();
    }

    pub fn end_frame(&mut self, vm: &mut dyn BxVM) -> Result<(), String> {
        let terminal = self.terminal.as_mut().ok_or("Terminal not initialized")?;

        let mut widgets_to_render = self.frame_widgets.clone();
        let canvas_cmds = self.canvas.clone();
        // Sort by Z-index
        widgets_to_render.sort_by_key(|w| w.5);

        let mut kitty_wanted: Vec<KittyPlacement> = Vec::new();
        vm.suspend_gc();
        let res = terminal
            .draw(|frame| {
                let term_area = frame.area();
                for (widget_id, x, y, width, height, _) in &widgets_to_render {
                    let rx = *x;
                    let ry = *y;

                    // Clip to terminal bounds to prevent panic in ratatui buffer
                    if rx >= term_area.width || ry >= term_area.height {
                        continue;
                    }

                    let rw = (*width).min(term_area.width.saturating_sub(rx));
                    let rh = (*height).min(term_area.height.saturating_sub(ry));

                    if rw > 0 && rh > 0 {
                        let area = ratatui::layout::Rect::new(rx, ry, rw, rh);
                        if let Some(widget) = WidgetRegistry::get(*widget_id) {
                            widget.render_in_area(vm, frame, area, &WidgetRegistry);
                        }
                    }
                }
                canvas::render_with(&canvas_cmds, frame.buffer_mut(), Some(&mut kitty_wanted));
                // Terminals without true colour get the nearest of the 256 palette colours.
                if !color::terminal_has_truecolor() {
                    color::downgrade(frame.buffer_mut());
                }
            })
            .map_err(|e| e.to_string());
        vm.resume_gc();

        res?;
        // Pictures drawn with the Kitty protocol go to the terminal after the cells, only when they changed.
        let kitty = &mut self.kitty;
        let bytes = imaging::guard(|| kitty.emit(&kitty_wanted, &|key| imaging::load(&key.path).map(|(_, img)| imaging::png_bytes(&img)).filter(|png| !png.is_empty()))).unwrap_or_default();
        if !bytes.is_empty() {
            if let Some(terminal) = self.terminal.as_mut() {
                use std::io::Write;
                let backend = terminal.backend_mut();
                backend.write_all(bytes.as_bytes()).map_err(|e| e.to_string())?;
                backend.flush().map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Text at a position in one foreground colour (any colour `color::parse` accepts), optionally bold.
    pub fn print(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        color: &str,
        bold: bool,
    ) -> Result<(), String> {
        let style = CellStyle { fg: Some(color::parse(color).ok_or_else(|| format!("Unknown colour: {color}"))?), bold, ..CellStyle::default() };
        self.canvas.push(DrawCmd::Text { x, y, text: text.to_string(), style });
        Ok(())
    }

    pub fn render_widget(
        &mut self,
        widget_id: usize,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        z_index: i32,
    ) {
        self.frame_widgets
            .push((widget_id, x, y, width, height, z_index));
    }

    pub fn hit_test(&self, x: u16, y: u16) -> Option<usize> {
        // Hit test should probably also respect Z-index (reverse order)
        let mut widgets = self.frame_widgets.clone();
        widgets.sort_by_key(|w| w.5);
        widgets.reverse();

        for (id, wx, wy, w, h, _) in &widgets {
            if x >= *wx && x < *wx + *w && y >= *wy && y < *wy + *h {
                return Some(*id);
            }
        }
        None
    }

    pub fn poll_key(&self, timeout_ms: u64) -> Result<String, String> {
        if event::poll(std::time::Duration::from_millis(timeout_ms)).map_err(|e| e.to_string())? {
            if let CrossTermEvent::Key(key) = event::read().map_err(|e| e.to_string())? {
                return Ok(format_key(key));
            }
        }
        Ok(String::new())
    }

    pub fn size(&self) -> Result<(u16, u16), String> {
        let (w, h) = terminal::size().map_err(|e| e.to_string())?;
        Ok((w, h))
    }

    pub fn clear(&mut self) -> Result<(), String> {
        let terminal = self.terminal.as_mut().ok_or("Terminal not initialized")?;
        terminal.clear().map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_mouse(&mut self, enabled: bool) -> Result<(), String> {
        if enabled && !self.mouse_enabled {
            execute!(io::stderr(), EnableMouseCaptureEvent).map_err(|e| e.to_string())?;
            self.mouse_enabled = true;
        } else if !enabled && self.mouse_enabled {
            execute!(io::stderr(), DisableMouseCaptureEvent).map_err(|e| e.to_string())?;
            self.mouse_enabled = false;
        }
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), String> {
        let clear = self.kitty.finish();
        if !clear.is_empty() {
            use std::io::Write;
            let _ = io::stderr().write_all(clear.as_bytes());
            let _ = io::stderr().flush();
        }
        if self.terminal.is_some() {
            terminal::disable_raw_mode().map_err(|e| e.to_string())?;
            execute!(io::stderr(), LeaveAlternateScreen, DisableMouseCaptureEvent)
                .map_err(|e| e.to_string())?;
            self.terminal = None;
            self.mouse_enabled = false;
        }
        Ok(())
    }
}

impl Drop for TUI {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// A key as text: the character itself, or a name (Enter, Tab, Escape, Up, F5, ...), with `Ctrl+` / `Alt+` in front
/// when held (and `Shift+` for named keys; a shifted character is already the capital letter).
fn format_key(key: KeyEvent) -> String {
    let (name, is_char) = match key.code {
        KeyCode::Char(c) => (c.to_string(), true),
        KeyCode::Enter => ("Enter".to_string(), false),
        KeyCode::Tab => ("Tab".to_string(), false),
        KeyCode::BackTab => ("BackTab".to_string(), false),
        KeyCode::Backspace => ("Backspace".to_string(), false),
        KeyCode::Esc => ("Escape".to_string(), false),
        KeyCode::Left => ("Left".to_string(), false),
        KeyCode::Right => ("Right".to_string(), false),
        KeyCode::Up => ("Up".to_string(), false),
        KeyCode::Down => ("Down".to_string(), false),
        KeyCode::Home => ("Home".to_string(), false),
        KeyCode::End => ("End".to_string(), false),
        KeyCode::PageUp => ("PageUp".to_string(), false),
        KeyCode::PageDown => ("PageDown".to_string(), false),
        KeyCode::Delete => ("Delete".to_string(), false),
        KeyCode::Insert => ("Insert".to_string(), false),
        KeyCode::F(n) => (format!("F{}", n), false),
        _ => ("?".to_string(), false),
    };
    let mut out = String::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        out.push_str("Ctrl+");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        out.push_str("Alt+");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) && !is_char && key.code != KeyCode::BackTab {
        out.push_str("Shift+");
    }
    out.push_str(&name);
    out
}

/// The style fields of a struct: `fg`, `bg` (colours), `bold`, `dim`, `italic`, `underline`. Null or `{}` is no style.
fn style_of(vm: &mut dyn BxVM, value: BxValue) -> Result<CellStyle, String> {
    let mut style = CellStyle::default();
    let Some(id) = value.as_gc_id() else { return Ok(style) };
    for key in ["fg", "bg"] {
        if vm.struct_key_exists(id, key) {
            let name = vm.to_string(vm.struct_get(id, key));
            let parsed = color::parse(&name).ok_or_else(|| format!("Unknown colour: {name}"))?;
            if key == "fg" { style.fg = Some(parsed) } else { style.bg = Some(parsed) }
        }
    }
    let flag = |vm: &mut dyn BxVM, key: &str| vm.struct_key_exists(id, key) && vm.is_truthy(vm.struct_get(id, key));
    style.bold = flag(vm, "bold");
    style.dim = flag(vm, "dim");
    style.italic = flag(vm, "italic");
    style.underline = flag(vm, "underline");
    Ok(style)
}

#[cfg(test)]
mod key_tests {
    use super::*;
    use crossterm::event::KeyEventState;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE }
    }

    #[test]
    fn plain_characters_and_names() {
        assert_eq!(format_key(key(KeyCode::Char('a'), KeyModifiers::NONE)), "a");
        assert_eq!(format_key(key(KeyCode::Char('A'), KeyModifiers::SHIFT)), "A");
        assert_eq!(format_key(key(KeyCode::Enter, KeyModifiers::NONE)), "Enter");
        assert_eq!(format_key(key(KeyCode::Esc, KeyModifiers::NONE)), "Escape");
        assert_eq!(format_key(key(KeyCode::F(5), KeyModifiers::NONE)), "F5");
        assert_eq!(format_key(key(KeyCode::BackTab, KeyModifiers::SHIFT)), "BackTab");
    }

    #[test]
    fn modifiers_are_part_of_the_name() {
        assert_eq!(format_key(key(KeyCode::Char('c'), KeyModifiers::CONTROL)), "Ctrl+c");
        assert_eq!(format_key(key(KeyCode::Char('x'), KeyModifiers::ALT)), "Alt+x");
        assert_eq!(format_key(key(KeyCode::Up, KeyModifiers::SHIFT)), "Shift+Up");
        assert_eq!(format_key(key(KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::ALT)), "Ctrl+Alt+Left");
    }
}
