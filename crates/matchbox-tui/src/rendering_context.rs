use matchbox_vm::{BxObject, bx_methods};

#[derive(Debug, Clone)]
pub enum DrawCommand {
    DrawText {
        x: u16,
        y: u16,
        text: String,
        color: Option<String>,
        z_index: i32,
    },
    DrawRect {
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        color: Option<String>,
        z_index: i32,
    },
}

impl DrawCommand {
    pub fn z_index(&self) -> i32 {
        match self {
            DrawCommand::DrawText { z_index, .. } => *z_index,
            DrawCommand::DrawRect { z_index, .. } => *z_index,
        }
    }

    pub fn playback(&self, frame: &mut ratatui::Frame) {
        use ratatui::layout::Rect;
        use ratatui::style::Style;
        use ratatui::widgets::{Block, Borders, Paragraph, Widget};
        use std::cmp::min;

        let frame_area = frame.area();

        match self {
            DrawCommand::DrawText {
                x, y, text, color, ..
            } => {
                if *x >= frame_area.width || *y >= frame_area.height || text.is_empty() {
                    return;
                }

                let area = Rect::new(
                    *x,
                    *y,
                    min(text.len() as u16, frame_area.width.saturating_sub(*x)),
                    1,
                );
                if area.width == 0 {
                    return;
                }

                let mut style = Style::default();
                if let Some(color_name) = color {
                    style = style.fg(parse_color(color_name));
                }
                let p = Paragraph::new(text.as_str()).style(style);
                p.render(area, frame.buffer_mut());
            }
            DrawCommand::DrawRect {
                x, y, w, h, color, ..
            } => {
                if *x >= frame_area.width || *y >= frame_area.height || *w == 0 || *h == 0 {
                    return;
                }

                let area = Rect::new(
                    *x,
                    *y,
                    min(*w, frame_area.width.saturating_sub(*x)),
                    min(*h, frame_area.height.saturating_sub(*y)),
                );
                if area.width == 0 || area.height == 0 {
                    return;
                }

                let mut block = Block::default().borders(Borders::ALL);
                if let Some(color_name) = color {
                    block = block.style(Style::default().fg(parse_color(color_name)));
                }
                block.render(area, frame.buffer_mut());
            }
        }
    }
}

fn parse_color(color: &str) -> ratatui::style::Color {
    use ratatui::style::Color;
    match color.to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        _ => Color::White,
    }
}

#[derive(Debug, BxObject)]
pub struct RenderingContext {
    pub commands: Vec<DrawCommand>,
    pub origin_stack: Vec<(u16, u16)>,
    pub current_origin: (u16, u16),
    pub current_z_index: i32,
}

impl RenderingContext {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            origin_stack: Vec::new(),
            current_origin: (0, 0),
            current_z_index: 0,
        }
    }

    pub fn playback(&mut self, frame: &mut ratatui::Frame) {
        self.commands.sort_by_key(|cmd| cmd.z_index());
        for cmd in &self.commands {
            cmd.playback(frame);
        }
    }
}

#[bx_methods]
#[allow(non_snake_case)]
impl RenderingContext {
    pub fn drawText(&mut self, x: f64, y: f64, text: String) {
        let actual_x = self.current_origin.0.saturating_add(x as u16);
        let actual_y = self.current_origin.1.saturating_add(y as u16);
        self.commands.push(DrawCommand::DrawText {
            x: actual_x,
            y: actual_y,
            text,
            color: None,
            z_index: self.current_z_index,
        });
    }

    pub fn drawRect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        let actual_x = self.current_origin.0.saturating_add(x as u16);
        let actual_y = self.current_origin.1.saturating_add(y as u16);
        self.commands.push(DrawCommand::DrawRect {
            x: actual_x,
            y: actual_y,
            w: w as u16,
            h: h as u16,
            color: None,
            z_index: self.current_z_index,
        });
    }

    pub fn pushOrigin(&mut self, x: f64, y: f64) {
        self.origin_stack.push(self.current_origin);
        self.current_origin.0 = self.current_origin.0.saturating_add(x as u16);
        self.current_origin.1 = self.current_origin.1.saturating_add(y as u16);
    }

    pub fn popOrigin(&mut self) {
        if let Some(old_origin) = self.origin_stack.pop() {
            self.current_origin = old_origin;
        }
    }

    pub fn setZIndex(&mut self, z: f64) {
        self.current_z_index = z as i32;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matchbox_vm::types::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    struct MockVM {
        interner: matchbox_vm::vm::intern::StringInterner,
    }

    impl MockVM {
        fn new() -> Self {
            Self {
                interner: matchbox_vm::vm::intern::StringInterner::new(),
            }
        }
    }

    impl BxVM for MockVM {
        fn current_chunk(&self) -> Option<Rc<RefCell<matchbox_vm::Chunk>>> {
            None
        }
        fn current_receiver(&self) -> Option<BxValue> {
            None
        }
        fn interpret_chunk(&mut self, _: matchbox_vm::Chunk) -> Result<BxValue, String> {
            Ok(BxValue::new_null())
        }
        fn spawn(
            &mut self,
            _: Rc<BxCompiledFunction>,
            _: Vec<BxValue>,
            _: u8,
            _: Rc<RefCell<matchbox_vm::Chunk>>,
        ) -> BxValue {
            BxValue::new_null()
        }
        fn spawn_by_value(
            &mut self,
            _: &BxValue,
            _: Vec<BxValue>,
            _: u8,
            _: Rc<RefCell<matchbox_vm::Chunk>>,
        ) -> Result<BxValue, String> {
            Ok(BxValue::new_null())
        }
        fn call_function_by_value(
            &mut self,
            _: &BxValue,
            _: Vec<BxValue>,
            _: Rc<RefCell<matchbox_vm::Chunk>>,
        ) -> Result<BxValue, String> {
            Ok(BxValue::new_null())
        }
        fn yield_fiber(&mut self) {}
        fn sleep(&mut self, _: u64) {}
        fn get_root_shape(&self) -> u32 {
            0
        }
        fn get_shape_index(&self, _: u32, _: &str) -> Option<u32> {
            None
        }
        fn get_len(&self, _: usize) -> usize {
            0
        }
        fn is_array_value(&self, _: BxValue) -> bool {
            false
        }
        fn is_struct_value(&self, _: BxValue) -> bool {
            false
        }
        fn is_string_value(&self, _: BxValue) -> bool {
            false
        }
        fn is_bytes(&self, _: BxValue) -> bool {
            false
        }
        fn bytes_new(&mut self, _: Vec<u8>) -> usize {
            0
        }
        fn bytes_len(&self, _: usize) -> usize {
            0
        }
        fn bytes_get(&self, _: usize, _: usize) -> Result<u8, String> {
            Err("not implemented".to_string())
        }
        fn bytes_set(&mut self, _: usize, _: usize, _: u8) -> Result<(), String> {
            Err("not implemented".to_string())
        }
        fn to_bytes(&self, _: BxValue) -> Result<Vec<u8>, String> {
            Err("not implemented".to_string())
        }
        fn array_len(&self, _: usize) -> usize {
            0
        }
        fn array_push(&mut self, _: usize, _: BxValue) {}
        fn array_pop(&mut self, _: usize) -> Result<BxValue, String> {
            Ok(BxValue::new_null())
        }
        fn array_get(&self, _: usize, _: usize) -> BxValue {
            BxValue::new_null()
        }
        fn array_set(&mut self, _: usize, _: usize, _: BxValue) -> Result<(), String> {
            Ok(())
        }
        fn array_delete_at(&mut self, _: usize, _: usize) -> Result<BxValue, String> {
            Ok(BxValue::new_null())
        }
        fn array_insert_at(&mut self, _: usize, _: usize, _: BxValue) -> Result<(), String> {
            Ok(())
        }
        fn array_clear(&mut self, _: usize) -> Result<(), String> {
            Ok(())
        }
        fn array_new(&mut self) -> usize {
            0
        }
        fn struct_len(&self, _: usize) -> usize {
            0
        }
        fn struct_new(&mut self) -> usize {
            0
        }
        fn struct_set(&mut self, _: usize, _: &str, _: BxValue) {}
        fn struct_get(&self, _: usize, _: &str) -> BxValue {
            BxValue::new_null()
        }
        fn struct_delete(&mut self, _: usize, _: &str) -> bool {
            false
        }
        fn struct_key_exists(&self, _: usize, _: &str) -> bool {
            false
        }
        fn struct_key_array(&self, _: usize) -> Vec<String> {
            vec![]
        }
        fn struct_clear(&mut self, _: usize) {}
        fn struct_get_shape(&self, _: usize) -> u32 {
            0
        }
        fn future_new(&mut self) -> BxValue {
            BxValue::new_null()
        }
        fn future_resolve(&mut self, _: BxValue, _: BxValue) -> Result<(), String> {
            Ok(())
        }
        fn future_reject(&mut self, _: BxValue, _: BxValue) -> Result<(), String> {
            Ok(())
        }
        fn future_schedule_resolve(&mut self, _: BxValue, _: BxValue) -> Result<(), String> {
            Ok(())
        }
        fn future_schedule_reject(&mut self, _: BxValue, _: BxValue) -> Result<(), String> {
            Ok(())
        }
        fn native_future_new(&mut self) -> NativeFutureHandle {
            let (tx, _rx) = std::sync::mpsc::channel();
            NativeFutureHandle::new(BxValue::new_null(), tx)
        }
        fn future_on_error(&mut self, _: usize, _: BxValue) {}
        fn native_object_new(&mut self, _: Rc<RefCell<dyn BxNativeObject>>) -> usize {
            0
        }
        fn native_object_call_method(
            &mut self,
            _: usize,
            _: &str,
            _: &[BxValue],
        ) -> Result<BxValue, String> {
            Ok(BxValue::new_null())
        }
        fn construct_native_class(&mut self, _: &str, _: &[BxValue]) -> Result<BxValue, String> {
            Ok(BxValue::new_null())
        }
        fn instance_class_name(&self, _: BxValue) -> Result<String, String> {
            Ok("Mock".to_string())
        }
        fn instance_variables_json(&self, _: BxValue) -> Result<serde_json::Value, String> {
            Ok(serde_json::json!({}))
        }
        fn string_new(&mut self, _: String) -> usize {
            0
        }
        fn to_string(&self, _v: BxValue) -> String {
            "".to_string()
        }
        fn to_box_string(&self, _: BxValue) -> box_string::BoxString {
            box_string::BoxString::new("")
        }
        fn insert_global(&mut self, _: String, _: BxValue) {}
        fn get_cli_args(&self) -> Vec<String> {
            vec![]
        }
        fn write_output(&mut self, _: &str) {}
        fn begin_output_capture(&mut self) {}
        fn end_output_capture(&mut self) -> Option<String> {
            Some(String::new())
        }
        fn suspend_gc(&mut self) {}
        fn resume_gc(&mut self) {}
        fn push_root(&mut self, _: BxValue) {}
        fn pop_root(&mut self) {}
        fn get_interner(&mut self) -> &mut matchbox_vm::vm::intern::StringInterner {
            &mut self.interner
        }
    }

    #[test]
    fn test_rendering_context_z_index_sorting() {
        let mut ctx = RenderingContext::new();
        ctx.setZIndex(10.0);
        ctx.drawText(0.0, 0.0, "Top".to_string());
        ctx.setZIndex(0.0);
        ctx.drawText(0.0, 0.0, "Bottom".to_string());

        assert_eq!(ctx.commands.len(), 2);
        assert_eq!(ctx.commands[0].z_index(), 10);

        ctx.commands.sort_by_key(|cmd| cmd.z_index());
        assert_eq!(ctx.commands[0].z_index(), 0);
        assert_eq!(ctx.commands[1].z_index(), 10);
    }

    #[test]
    fn test_playback_clips_commands_to_frame_bounds() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let backend = TestBackend::new(10, 4);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut ctx = RenderingContext::new();
        ctx.commands.push(DrawCommand::DrawText {
            x: 8,
            y: 4,
            text: "overflow".to_string(),
            color: None,
            z_index: 0,
        });
        ctx.commands.push(DrawCommand::DrawRect {
            x: 9,
            y: 3,
            w: 5,
            h: 5,
            color: None,
            z_index: 0,
        });

        terminal
            .draw(|frame| {
                ctx.playback(frame);
            })
            .unwrap();
    }
}
