#[cfg(test)]
mod tests {
    use crate::{TUIApp, TUI};
    use matchbox_vm::types::*;
    use std::rc::Rc;
    use std::cell::RefCell;
    use crate::widget::{WidgetRegistry, WidgetKind};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    struct MockVM {
        pub last_method_called: String,
        pub last_args: Vec<BxValue>,
        pub strings: Vec<String>,
    }
    
    impl MockVM {
        fn new() -> Self {
            Self {
                last_method_called: String::new(),
                last_args: Vec::new(),
                strings: Vec::new(),
            }
        }
    }

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
        fn native_object_new(&mut self, _: Rc<RefCell<dyn BxNativeObject>>) -> usize { 42 }
        fn native_object_call_method(&mut self, _: usize, name: &str, args: &[BxValue]) -> Result<BxValue, String> { 
            self.last_method_called = name.to_string();
            self.last_args = args.to_vec();
            Ok(BxValue::new_null()) 
        }
        fn construct_native_class(&mut self, _: &str, _: &[BxValue]) -> Result<BxValue, String> { Ok(BxValue::new_null()) }
        fn string_new(&mut self, s: String) -> usize { 
            let id = self.strings.len();
            self.strings.push(s);
            id
        }
        fn to_string(&self, v: BxValue) -> String { 
            if let Some(id) = v.as_gc_id() {
                if id < self.strings.len() {
                    return self.strings[id].clone();
                }
            }
            "".to_string() 
        }
        fn to_box_string(&self, _: BxValue) -> box_string::BoxString { box_string::BoxString::new("") }
        fn get_cli_args(&self) -> Vec<String> { vec![] }
        fn write_output(&mut self, _: &str) {}
    }

    #[test]
    fn test_text_widget_fluent_api() {
        let mut vm = MockVM::new();
        let mut widget = crate::TextWidget {
            text: String::new(),
            alignment: crate::widget::TextAlignment::Left,
            wrap: false,
            fg_color: None,
            bold: false,
            italic: false,
            underline: false,
        };
        
        // Test fluent methods via Rust directly first
        widget.text("Hello".to_string())
              .color("red".to_string())
              .bold(true);
              
        assert_eq!(widget.text, "Hello");
        assert_eq!(widget.fg_color, Some("red".to_string()));
        assert!(widget.bold);
        
        // Test via call_method (as BoxLang would)
        let world_id = vm.string_new("World".to_string());
        widget.call_method(&mut vm, 0, "text", &[BxValue::new_ptr(world_id)]).unwrap();
        assert_eq!(widget.text, "World");
    }

    #[test]
    fn test_text_widget_render() {
        let mut vm = MockVM::new();
        let mut widget = crate::TextWidget {
            text: "Hello".to_string(),
            alignment: crate::widget::TextAlignment::Left,
            wrap: false,
            fg_color: None,
            bold: false,
            italic: false,
            underline: false,
        };
        
        let ctx_id = 42;
        widget.__render(&mut vm, BxValue::new_ptr(ctx_id)).unwrap();
        
        assert_eq!(vm.last_method_called, "drawText");
        assert_eq!(vm.last_args.len(), 3);
        assert_eq!(vm.to_string(vm.last_args[2]), "Hello");
    }

    #[test]
    fn test_custom_widget_rendering() {
        let mut vm = MockVM::new();
        let custom_obj = BxValue::new_ptr(123);
        let widget = WidgetKind::Custom(custom_obj);
        let registry = WidgetRegistry::new();
        
        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        
        terminal.draw(|frame| {
            let area = ratatui::layout::Rect::new(0, 0, 10, 5);
            widget.render_in_area(&mut vm, frame, area, &registry);
        }).unwrap();
        
        assert_eq!(vm.last_method_called, "__render");
        assert_eq!(vm.last_args.len(), 2);
        assert!(vm.last_args[0].is_ptr()); // ctx
        assert!(vm.last_args[1].is_ptr()); // area
    }

    #[test]
    fn test_tui_dirty_flag() {
        TUI::with_current(|tui| {
            tui.set_dirty_val(false);
            assert!(!tui.is_dirty());
            tui.set_dirty();
            assert!(tui.is_dirty());
        });
    }

    #[test]
    fn test_tui_app_stop() {
        let mut app = TUIApp { quit: false };
        assert!(!app.quit);
        app.stop();
        assert!(app.quit);
    }
}
