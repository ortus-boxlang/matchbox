use matchbox_vm::types::BxNativeObject;
use matchbox_vm::{BxObject, bx_methods};

#[derive(Debug, Clone)]
pub enum DrawCommand {
    DrawText { x: u16, y: u16, text: String, color: Option<String> },
    DrawRect { x: u16, y: u16, w: u16, h: u16, color: Option<String> },
}

impl DrawCommand {
    pub fn playback(&self, frame: &mut ratatui::Frame) {
        use ratatui::widgets::{Widget, Paragraph, Block, Borders};
        use ratatui::layout::Rect;
        use ratatui::style::{Style, Color};

        match self {
            DrawCommand::DrawText { x, y, text, color } => {
                let area = Rect::new(*x, *y, text.len() as u16, 1);
                let mut style = Style::default();
                if let Some(color_name) = color {
                    style = style.fg(parse_color(color_name));
                }
                let p = Paragraph::new(text.as_str()).style(style);
                p.render(area, frame.buffer_mut());
            }
            DrawCommand::DrawRect { x, y, w, h, color } => {
                let area = Rect::new(*x, *y, *w, *h);
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
}

impl RenderingContext {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            origin_stack: Vec::new(),
            current_origin: (0, 0),
        }
    }

    pub fn playback(&self, frame: &mut ratatui::Frame) {
        for cmd in &self.commands {
            cmd.playback(frame);
        }
    }
}

#[bx_methods]
#[allow(non_snake_case)]
impl RenderingContext {
    pub fn drawText(&mut self, x: i32, y: i32, text: String) {
        let actual_x = self.current_origin.0 + x as u16;
        let actual_y = self.current_origin.1 + y as u16;
        self.commands.push(DrawCommand::DrawText {
            x: actual_x,
            y: actual_y,
            text,
            color: None,
        });
    }

    pub fn drawRect(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let actual_x = self.current_origin.0 + x as u16;
        let actual_y = self.current_origin.1 + y as u16;
        self.commands.push(DrawCommand::DrawRect {
            x: actual_x,
            y: actual_y,
            w: w as u16,
            h: h as u16,
            color: None,
        });
    }

    pub fn pushOrigin(&mut self, x: i32, y: i32) {
        self.origin_stack.push(self.current_origin);
        self.current_origin.0 += x as u16;
        self.current_origin.1 += y as u16;
    }

    pub fn popOrigin(&mut self) {
        if let Some(old_origin) = self.origin_stack.pop() {
            self.current_origin = old_origin;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matchbox_vm::types::*;
    use std::rc::Rc;
    use std::cell::RefCell;
    
    struct MockVM;
    impl BxVM for MockVM {
        fn current_chunk(&self) -> Option<Rc<RefCell<matchbox_vm::Chunk>>> { None }
        fn spawn(&mut self, _: Rc<BxCompiledFunction>, _: Vec<BxValue>, _: u8, _: Rc<RefCell<matchbox_vm::Chunk>>) -> BxValue { BxValue::new_null() }
        fn spawn_by_value(&mut self, _: &BxValue, _: Vec<BxValue>, _: u8, _: Rc<RefCell<matchbox_vm::Chunk>>) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn call_function_by_value(&mut self, _: &BxValue, _: Vec<BxValue>, _: Rc<RefCell<matchbox_vm::Chunk>>) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn yield_fiber(&mut self) {}
        fn sleep(&mut self, _: u64) {}
        fn get_root_shape(&self) -> u32 { 0 }
        fn get_shape_index(&self, _: u32, _: &str) -> Option<u32> { None }
        fn get_len(&self, _: usize) -> usize { 0 }
        fn array_len(&self, _: usize) -> usize { 0 }
        fn array_push(&mut self, _: usize, _: BxValue) {}
        fn array_pop(&mut self, _: usize) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn array_get(&self, _: usize, _: usize) -> BxValue { BxValue::new_null() }
        fn array_set(&mut self, _: usize, _: usize, _: BxValue) -> Result<(), String> { Ok(()) }
        fn array_delete_at(&mut self, _: usize, _: usize) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn array_insert_at(&mut self, _: usize, _: usize, _: BxValue) -> Result<(), String> { Ok(()) }
        fn array_clear(&mut self, _: usize) -> Result<(), String> { Ok(()) }
        fn array_new(&mut self) -> usize { 0 }
        fn struct_len(&self, _: usize) -> usize { 0 }
        fn struct_new(&mut self) -> usize { 0 }
        fn struct_set(&mut self, _: usize, _: &str, _: BxValue) {}
        fn struct_get(&self, _: usize, _: &str) -> BxValue { BxValue::new_null() }
        fn struct_delete(&mut self, _: usize, _: &str) -> bool { false }
        fn struct_key_exists(&self, _: usize, _: &str) -> bool { false }
        fn struct_key_array(&self, _: usize) -> Vec<String> { vec![] }
        fn struct_clear(&mut self, _: usize) {}
        fn struct_get_shape(&self, _: usize) -> u32 { 0 }
        fn future_on_error(&mut self, _: usize, _: BxValue) {}
        fn native_object_new(&mut self, _: Rc<RefCell<dyn BxNativeObject>>) -> usize { 0 }
        fn native_object_call_method(&mut self, _: usize, _: &str, _: &[BxValue]) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn construct_native_class(&mut self, _: &str, _: &[BxValue]) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn string_new(&mut self, _: String) -> usize { 0 }
        fn to_string(&self, _v: BxValue) -> String { 
             "".to_string() 
        }
        fn to_box_string(&self, _: BxValue) -> box_string::BoxString { box_string::BoxString::new("") }
        fn get_cli_args(&self) -> Vec<String> { vec![] }
        fn write_output(&mut self, _: &str) {}
    }

    #[test]
    fn test_rendering_context_commands() {
        let mut ctx = RenderingContext::new();
        ctx.drawText(10, 5, "Hello".to_string());
        ctx.drawRect(0, 0, 20, 10);
        
        assert_eq!(ctx.commands.len(), 2);
        
        if let DrawCommand::DrawText { x, y, text, .. } = &ctx.commands[0] {
            assert_eq!(*x, 10);
            assert_eq!(*y, 5);
            assert_eq!(text, "Hello");
        } else {
            panic!("Expected DrawText");
        }
        
        if let DrawCommand::DrawRect { x, y, w, h, .. } = &ctx.commands[1] {
            assert_eq!(*x, 0);
            assert_eq!(*y, 0);
            assert_eq!(*w, 20);
            assert_eq!(*h, 10);
        } else {
            panic!("Expected DrawRect");
        }
    }

    #[test]
    fn test_rendering_context_origin() {
        let mut ctx = RenderingContext::new();
        ctx.pushOrigin(5, 5);
        ctx.drawText(2, 2, "World".to_string());
        
        if let DrawCommand::DrawText { x, y, .. } = &ctx.commands[0] {
            assert_eq!(*x, 7);
            assert_eq!(*y, 7);
        }
        
        ctx.pushOrigin(10, 0);
        ctx.drawText(1, 1, "Nested".to_string());
        if let DrawCommand::DrawText { x, y, .. } = &ctx.commands[1] {
            assert_eq!(*x, 16);
            assert_eq!(*y, 6);
        }
        
        ctx.popOrigin();
        ctx.drawText(1, 1, "Back".to_string());
        if let DrawCommand::DrawText { x, y, .. } = &ctx.commands[2] {
            assert_eq!(*x, 6);
            assert_eq!(*y, 6);
        }
        
        ctx.popOrigin();
        ctx.drawText(0, 0, "Root".to_string());
        if let DrawCommand::DrawText { x, y, .. } = &ctx.commands[3] {
            assert_eq!(*x, 0);
            assert_eq!(*y, 0);
        }
    }
}
