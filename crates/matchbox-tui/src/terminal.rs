use std::cell::RefCell;
use std::io;

use crossterm::event::DisableMouseCapture as DisableMouseCaptureEvent;
use crossterm::event::EnableMouseCapture as EnableMouseCaptureEvent;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use matchbox_vm::types::{BxNativeObject, BxVM, BxValue};
use matchbox_vm::{BxObject, bx_methods};

use crate::widget::{TextAlignment, TextWidget, WidgetKind, WidgetRegistry};

#[derive(BxObject)]
pub struct TUI {
    terminal: Option<Terminal<CrosstermBackend<io::Stdout>>>,
    frame_widgets: Vec<(usize, u16, u16, u16, u16)>,
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
    pub fn bx_init(&mut self) -> Result<(), String> {
        self.init()
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
        vm: &mut dyn BxVM,
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
        );
    }

    pub fn bx_get_key(&self, vm: &mut dyn BxVM) -> Result<BxValue, String> {
        let key = self.get_key()?;
        Ok(BxValue::new_ptr(vm.string_new(key)))
    }

    pub fn bx_poll_key(&self, vm: &mut dyn BxVM, timeout: f64) -> Result<BxValue, String> {
        let key = self.poll_key(timeout as u64)?;
        Ok(BxValue::new_ptr(vm.string_new(key)))
    }

    pub fn bx_size(&self, vm: &mut dyn BxVM) -> Result<BxValue, String> {
        let (w, h) = self.size()?;
        let s = vm.struct_new();
        vm.struct_set(s, "width", BxValue::new_number(w as f64));
        vm.struct_set(s, "height", BxValue::new_number(h as f64));
        Ok(BxValue::new_ptr(s))
    }

    pub fn bx_clear(&mut self) -> Result<(), String> {
        self.clear()
    }

    pub fn bx_set_mouse(&mut self, enabled: bool) -> Result<(), String> {
        self.set_mouse(enabled)
    }

    pub fn bx_shutdown(&mut self) -> Result<(), String> {
        self.shutdown()
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
        CURRENT_TUI.with(|tui| f(&mut tui.borrow_mut()))
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
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCaptureEvent)
            .map_err(|e| e.to_string())?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
        self.terminal = Some(terminal);
        Ok(())
    }

    pub fn begin_frame(&mut self) {
        self.frame_widgets.clear();
    }

    pub fn end_frame(&mut self, vm: &mut dyn BxVM) -> Result<(), String> {
        let terminal = self.terminal.as_mut().ok_or("Terminal not initialized")?;

        let widgets_to_render: Vec<(usize, u16, u16, u16, u16)> = self.frame_widgets.clone();

        terminal
            .draw(|frame| {
                for (widget_id, x, y, width, height) in &widgets_to_render {
                    let area = ratatui::layout::Rect::new(*x, *y, *width, *height);
                    WidgetRegistry::with_current(|registry| {
                        if let Some(widget) = registry.get(*widget_id) {
                            widget.render_in_area(vm, frame, area, registry);
                        }
                    });
                }
            })
            .map_err(|e| e.to_string())?;
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
        let widget = TextWidget {
            text: text.to_string(),
            alignment: TextAlignment::Left,
            wrap: false,
            fg_color: Some(color.to_string()),
            bold,
            italic: false,
            underline: false,
        };

        let widget_id = WidgetRegistry::with_current(|r| r.insert(WidgetKind::Text(widget)));
        let width = text.len() as u16;
        self.frame_widgets.push((widget_id, x, y, width.max(1), 1));
        Ok(())
    }

    pub fn render_widget(&mut self, widget_id: usize, x: u16, y: u16, width: u16, height: u16) {
        self.frame_widgets.push((widget_id, x, y, width, height));
    }

    pub fn get_key(&self) -> Result<String, String> {
        if event::poll(std::time::Duration::from_millis(100)).map_err(|e| e.to_string())? {
            if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
                return Ok(format_key(key));
            }
        }
        Ok(String::new())
    }

    pub fn poll_key(&self, timeout_ms: u64) -> Result<String, String> {
        if event::poll(std::time::Duration::from_millis(timeout_ms)).map_err(|e| e.to_string())? {
            if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
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
            execute!(io::stdout(), EnableMouseCaptureEvent).map_err(|e| e.to_string())?;
            self.mouse_enabled = true;
        } else if !enabled && self.mouse_enabled {
            execute!(io::stdout(), DisableMouseCaptureEvent).map_err(|e| e.to_string())?;
            self.mouse_enabled = false;
        }
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), String> {
        if self.terminal.is_some() {
            terminal::disable_raw_mode().map_err(|e| e.to_string())?;
            execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCaptureEvent)
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
