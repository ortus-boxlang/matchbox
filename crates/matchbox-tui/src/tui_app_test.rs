#[cfg(test)]
mod tests {
    use crate::widget::{WidgetKind, WidgetRegistry};
    use crate::{TUI, TUIApp};
    use matchbox_vm::types::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    struct MockVM {
        pub last_method_called: String,
        pub method_calls: Vec<String>,
        pub last_args: Vec<BxValue>,
        pub strings: Vec<String>,
        pub spawn_called: bool,
        pub structs: HashMap<usize, HashMap<String, BxValue>>,
        pub next_id: usize,
        pub interner: matchbox_vm::vm::intern::StringInterner,
    }

    impl MockVM {
        fn new() -> Self {
            Self {
                last_method_called: String::new(),
                method_calls: Vec::new(),
                last_args: Vec::new(),
                strings: Vec::new(),
                spawn_called: false,
                structs: HashMap::new(),
                next_id: 1,
                interner: matchbox_vm::vm::intern::StringInterner::new(),
            }
        }
    }

    impl BxVM for MockVM {
        fn current_chunk(&self) -> Option<Rc<RefCell<matchbox_vm::Chunk>>> {
            Some(Rc::new(RefCell::new(matchbox_vm::Chunk::new("test.bxs"))))
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
            self.spawn_called = true;
            BxValue::new_null()
        }
        fn spawn_by_value(
            &mut self,
            _: &BxValue,
            _: Vec<BxValue>,
            _: u8,
            _: Rc<RefCell<matchbox_vm::Chunk>>,
        ) -> Result<BxValue, String> {
            self.spawn_called = true;
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
        fn is_string_value(&self, val: BxValue) -> bool {
            val.as_gc_id()
                .map(|id| id < self.strings.len())
                .unwrap_or(false)
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
        fn struct_len(&self, id: usize) -> usize {
            self.structs.get(&id).map(|s| s.len()).unwrap_or(0)
        }
        fn struct_new(&mut self) -> usize {
            let id = self.next_id;
            self.next_id += 1;
            self.structs.insert(id, HashMap::new());
            id
        }
        fn struct_set(&mut self, id: usize, key: &str, val: BxValue) {
            self.structs
                .entry(id)
                .or_default()
                .insert(key.to_string(), val);
        }
        fn struct_get(&self, id: usize, key: &str) -> BxValue {
            self.structs
                .get(&id)
                .and_then(|s| s.get(key).copied())
                .unwrap_or(BxValue::new_null())
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
            42
        }
        fn native_object_call_method(
            &mut self,
            _: usize,
            name: &str,
            args: &[BxValue],
        ) -> Result<BxValue, String> {
            self.last_method_called = name.to_string();
            self.method_calls.push(name.to_string());
            self.last_args = args.to_vec();
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
            z_index: 0,
        };

        // Test fluent methods via Rust directly first
        widget
            .text("Hello".to_string())
            .color("red".to_string())
            .bold(true);

        assert_eq!(widget.text, "Hello");
        assert_eq!(widget.fg_color, Some("red".to_string()));
        assert!(widget.bold);

        // Test via call_method (as BoxLang would)
        let world_id = vm.string_new("World".to_string());
        widget
            .call_method(&mut vm, 0, "text", &[BxValue::new_ptr(world_id)])
            .unwrap();
        assert_eq!(widget.text, "World");
    }

    #[test]
    fn test_text_widget_render() {
        let mut vm = MockVM::new();
        let widget = crate::TextWidget {
            text: "Hello".to_string(),
            alignment: crate::widget::TextAlignment::Left,
            wrap: false,
            fg_color: None,
            bold: false,
            italic: false,
            underline: false,
            z_index: 0,
        };

        let ctx_id = 42;
        let area_id = vm.struct_new();
        widget
            .__render(&mut vm, BxValue::new_ptr(ctx_id), BxValue::new_ptr(area_id))
            .unwrap();

        assert_eq!(vm.last_method_called, "drawText");
        assert_eq!(vm.last_args.len(), 3);
        assert_eq!(vm.to_string(vm.last_args[2]), "Hello");
    }

    #[test]
    fn test_custom_widget_rendering() {
        let mut vm = MockVM::new();
        let custom_obj = BxValue::new_ptr(123);
        let widget = WidgetKind::Custom(custom_obj);

        let backend = TestBackend::new(20, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let area = ratatui::layout::Rect::new(0, 0, 10, 5);
                widget.render_in_area(&mut vm, frame, area, &WidgetRegistry);
            })
            .unwrap();

        assert_eq!(vm.last_method_called, "__render");
        assert_eq!(vm.last_args.len(), 2);
        assert!(vm.last_args[0].is_ptr()); // ctx
        assert!(vm.last_args[1].is_ptr()); // area
    }

    #[test]
    fn test_vbox_layout() {
        let mut vm = MockVM::new();
        let mut vbox = crate::widget::VBoxWidget {
            children: Vec::new(),
            z_index: 0,
        };

        // Add two dummy children (just IDs)
        vbox.add(BxValue::new_ptr(101)).add(BxValue::new_ptr(102));

        let ctx_id = 42;
        let area_id = vm.struct_new();
        vm.struct_set(area_id, "x", BxValue::new_number(0.0));
        vm.struct_set(area_id, "y", BxValue::new_number(0.0));
        vm.struct_set(area_id, "w", BxValue::new_number(10.0));
        vm.struct_set(area_id, "h", BxValue::new_number(10.0));

        vbox.__render(&mut vm, BxValue::new_ptr(ctx_id), BxValue::new_ptr(area_id))
            .unwrap();

        assert!(vm.method_calls.iter().any(|name| name == "__render"));
    }

    #[test]
    fn test_hbox_layout() {
        let mut vm = MockVM::new();
        let mut hbox = crate::widget::HBoxWidget {
            children: Vec::new(),
            z_index: 0,
        };

        hbox.add(BxValue::new_ptr(101));

        let ctx_id = 42;
        let area_id = vm.struct_new();
        vm.struct_set(area_id, "x", BxValue::new_number(0.0));
        vm.struct_set(area_id, "y", BxValue::new_number(0.0));
        vm.struct_set(area_id, "w", BxValue::new_number(10.0));
        vm.struct_set(area_id, "h", BxValue::new_number(10.0));

        hbox.__render(&mut vm, BxValue::new_ptr(ctx_id), BxValue::new_ptr(area_id))
            .unwrap();

        assert!(vm.method_calls.iter().any(|name| name == "__render"));
    }

    #[test]
    fn test_button_click_event() {
        let button = crate::widget::ButtonWidget {
            label: "Click Me".to_string(),
            on_click: Some(BxValue::new_ptr(999)),
            z_index: 0,
        };

        let id = WidgetRegistry::insert(WidgetKind::Button(button.clone()));

        TUI::with_current(|tui| {
            tui.begin_frame();
            // Simulate that the button was rendered at (0,0,10,1)
            tui.render_widget(id, 0, 0, 10, 1, 0);

            // Hit test
            assert_eq!(tui.hit_test(5, 0), Some(id));
        });

        assert!(button.on_click.is_some());
    }

    #[test]
    fn test_z_index_sorting() {
        TUI::with_current(|tui| {
            tui.bx_begin_frame();
            // Add a background widget at Z=0
            let w_bg = 1;
            tui.render_widget(w_bg, 0, 0, 10, 10, 0);

            // Add an overlay widget at Z=10
            let w_fg = 2;
            tui.render_widget(w_fg, 0, 0, 10, 10, 10);

            // Hit test at (5,5) should return w_fg (the overlay)
            // because hit_test sorts by Z-index descending.
            assert_eq!(tui.hit_test(5, 5), Some(w_fg));
        });
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
