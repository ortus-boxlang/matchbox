use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use crossterm::event::DisableMouseCapture as DisableMouseCaptureEvent;
use crossterm::event::EnableMouseCapture as EnableMouseCaptureEvent;
use crossterm::event::{self, Event as CrossTermEvent, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use matchbox_vm::types::{BxVM, BxValue, Tracer, BxNativeObject};
use matchbox_vm::bx_methods;

use crate::widget::{WidgetKind, WidgetRegistry};

pub struct TUI {
    terminal: Option<Terminal<CrosstermBackend<io::Stderr>>>,
    frame_widgets: Vec<(usize, u16, u16, u16, u16, i32)>,
    mouse_enabled: bool,
    dirty: bool,
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

    pub fn bx_poll_event(&self, vm: &mut dyn BxVM, timeout: f64) -> Result<BxValue, String> {
        if event::poll(std::time::Duration::from_millis(timeout as u64)).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                CrossTermEvent::Key(key) => {
                    let s = vm.struct_new();
                    let type_id = vm.string_new("key".to_string());
                    vm.struct_set(s, "type", BxValue::new_ptr(type_id));
                    let key_str = format_key(key);
                    let key_id = vm.string_new(key_str);
                    vm.struct_set(s, "key", BxValue::new_ptr(key_id));
                    Ok(BxValue::new_ptr(s))
                }
                CrossTermEvent::Mouse(mouse) => {
                    if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                        let s = vm.struct_new();
                        let type_id = vm.string_new("click".to_string());
                        vm.struct_set(s, "type", BxValue::new_ptr(type_id));
                        let x = mouse.column as f64;
                        let y = mouse.row as f64;
                        vm.struct_set(s, "x", BxValue::new_number(x));
                        vm.struct_set(s, "y", BxValue::new_number(y));
                        
                        // Hit test
                        if let Some(widget_id) = self.hit_test(mouse.column, mouse.row) {
                            vm.struct_set(s, "widgetId", BxValue::new_number(widget_id as f64));
                        }
                        
                        Ok(BxValue::new_ptr(s))
                    } else {
                        Ok(BxValue::new_null())
                    }
                }
                _ => Ok(BxValue::new_null()),
            }
        } else {
            Ok(BxValue::new_null())
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

    fn call_method(&mut self, vm: &mut dyn BxVM, id: usize, name: &str, args: &[BxValue]) -> Result<BxValue, String> {
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
            mouse_enabled: false,
            dirty: true,
        }
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
    }

    pub fn end_frame(&mut self, vm: &mut dyn BxVM) -> Result<(), String> {
        let terminal = self.terminal.as_mut().ok_or("Terminal not initialized")?;

        let mut widgets_to_render = self.frame_widgets.clone();
        // Sort by Z-index
        widgets_to_render.sort_by_key(|w| w.5);

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
            })
            .map_err(|e| e.to_string());
        vm.resume_gc();
        
        res?;
        Ok(())
    }

    pub fn print(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        color: &str,
        bold: bool,
    ) -> Result<(), String> {
        use crate::widget::{TextAlignment, TextWidget};
        let widget = TextWidget {
            text: text.to_string(),
            alignment: TextAlignment::Left,
            wrap: false,
            fg_color: Some(color.to_string()),
            bold,
            italic: false,
            underline: false,
            z_index: 0,
        };

        let widget_id = WidgetRegistry::insert(WidgetKind::Text(widget));
        let width = text.len() as u16;
        self.frame_widgets.push((widget_id, x, y, width.max(1), 1, 0));
        Ok(())
    }

    pub fn render_widget(&mut self, widget_id: usize, x: u16, y: u16, width: u16, height: u16, z_index: i32) {
        self.frame_widgets.push((widget_id, x, y, width, height, z_index));
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

fn format_key(key: KeyEvent) -> String {
    match key.code {
        KeyCode::Char(c) => return c.to_string(),
        KeyCode::Enter => return "Enter".to_string(),
        KeyCode::Tab => return "Tab".to_string(),
        KeyCode::BackTab => return "BackTab".to_string(),
        KeyCode::Backspace => return "Backspace".to_string(),
        KeyCode::Esc => return "Escape".to_string(),
        KeyCode::Left => return "Left".to_string(),
        KeyCode::Right => return "Right".to_string(),
        KeyCode::Up => return "Up".to_string(),
        KeyCode::Down => return "Down".to_string(),
        KeyCode::Home => return "Home".to_string(),
        KeyCode::End => return "End".to_string(),
        KeyCode::PageUp => return "PageUp".to_string(),
        KeyCode::PageDown => return "PageDown".to_string(),
        KeyCode::Delete => return "Delete".to_string(),
        KeyCode::Insert => return "Insert".to_string(),
        KeyCode::F(n) => return format!("F{}", n),
        _ => {}
    }

    let mut result = String::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        result.push_str("Ctrl+");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        result.push_str("Alt+");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        result.push_str("Shift+");
    }
    match key.code {
        KeyCode::Char(c) => result.push(c),
        _ => result.push('?'),
    }
    result
}
