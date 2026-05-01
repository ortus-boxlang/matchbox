use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use matchbox_vm::types::{BxNativeFunction, BxVM, BxValue};
use matchbox_vm::{BxObject, bx_methods};

mod terminal;
mod widget;
pub mod rendering_context;

#[cfg(test)]
mod tui_app_test;

pub use terminal::TUI;
pub use widget::{
    BlockWidget, BorderType, InputWidget, ListStyle, ListWidget, ProgressBarWidget, TableColumn,
    TableWidget, TextAlignment, TextWidget, WidgetKind, WidgetRegistry,
    VBoxWidget, HBoxWidget, ButtonWidget
};

#[derive(Debug, BxObject)]
pub struct TUIApp {
    pub quit: bool,
}

#[bx_methods]
impl TUIApp {
    pub fn bx_run(&mut self, vm: &mut dyn BxVM) -> Result<(), String> {
        self.run(vm)
    }

    pub fn bx_is_quit(&self) -> bool {
        self.quit
    }

    pub fn bx_stop(&mut self) {
        self.quit = true;
    }
}

impl TUIApp {
    pub fn run(&mut self, vm: &mut dyn BxVM) -> Result<(), String> {
        TUI::with_current(|tui| tui.init())?;

        while !self.quit {
            // 1. Poll for events
            let event_res = TUI::with_current(|tui| tui.bx_poll_event(vm, 10.0));
            if let Ok(event_val) = event_res {
                if !event_val.is_null() {
                    if let Some(event_id) = event_val.as_gc_id() {
                        let event_type = vm.to_string(vm.struct_get(event_id, "type"));
                        
                        if event_type == "key" {
                            let key = vm.to_string(vm.struct_get(event_id, "key"));
                            if key == "Ctrl+c" {
                                self.quit = true;
                            }
                        } else if event_type == "click" {
                            let widget_id_val = vm.struct_get(event_id, "widgetId");
                            if !widget_id_val.is_null() {
                                let widget_id = widget_id_val.as_number() as usize;
                                if let Some(WidgetKind::Button(button)) = WidgetRegistry::get(widget_id) {
                                    if let Some(callback) = button.on_click {
                                        // Spawn a fiber for the click handler
                                        if let Some(chunk) = vm.current_chunk() {
                                            let _ = vm.spawn_by_value(&callback, vec![], 0, chunk);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 2. Check dirty and render
            let is_dirty = TUI::with_current(|tui| tui.is_dirty());
            if is_dirty {
                TUI::with_current(|tui| {
                    tui.begin_frame();
                    let _ = tui.end_frame(vm);
                    tui.set_dirty_val(false);
                });
            }

            // 3. Yield to other fibers
            vm.yield_fiber();
        }

        TUI::with_current(|tui| tui.shutdown())?;
        Ok(())
    }

    pub fn stop(&mut self) {
        self.quit = true;
    }
}

pub fn create_tui_app(_vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let app = TUIApp { quit: false };
    Ok(BxValue::new_ptr(
        matchbox_vm::types::BxVM::native_object_new(_vm, Rc::new(RefCell::new(app))),
    ))
}

pub fn create_tui(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let tui = TUI::new();
    let id = vm.native_object_new(Rc::new(RefCell::new(tui)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_text_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = TextWidget {
        text: String::new(),
        alignment: TextAlignment::Left,
        wrap: false,
        fg_color: None,
        bold: false,
        italic: false,
        underline: false,
        z_index: 0,
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_list_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = ListWidget {
        items: Vec::new(),
        selected: 0,
        style: ListStyle::Plain,
        highlight_symbol: None,
        z_index: 0,
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_table_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = TableWidget {
        columns: Vec::new(),
        rows: Vec::new(),
        selected: 0,
        show_header: true,
        column_widths: None,
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_block_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = BlockWidget {
        title: String::new(),
        border_type: BorderType::Plain,
        inner_widget: None,
        z_index: 0,
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_input_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = InputWidget {
        value: String::new(),
        placeholder: String::new(),
        prompt: String::new(),
        z_index: 0,
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_custom_widget(_vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("Custom widget requires an object".to_string());
    }
    let widget = WidgetKind::Custom(args[0]);
    let id = WidgetRegistry::insert(widget);
    Ok(BxValue::new_number(id as f64))
}

pub fn create_vbox_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = crate::widget::VBoxWidget {
        children: Vec::new(),
        z_index: 0,
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_hbox_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = crate::widget::HBoxWidget {
        children: Vec::new(),
        z_index: 0,
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_button_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = crate::widget::ButtonWidget {
        label: String::new(),
        on_click: None,
        z_index: 0,
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_progress_bar_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = ProgressBarWidget {
        completed: 0,
        total: 0,
        start_color: None,
        end_color: None,
        empty_color: None,
        show_label: true,
        label_position: "center".to_string(),
        fill_char: None,
        empty_char: None,
        z_index: 0,
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn register_classes() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
    map.insert("tui.App".to_string(), create_tui_app as BxNativeFunction);
    map.insert("tui.TUI".to_string(), create_tui as BxNativeFunction);
    map.insert(
        "tui.Text".to_string(),
        create_text_widget as BxNativeFunction,
    );
    map.insert(
        "tui.List".to_string(),
        create_list_widget as BxNativeFunction,
    );
    map.insert(
        "tui.Table".to_string(),
        create_table_widget as BxNativeFunction,
    );
    map.insert(
        "tui.Block".to_string(),
        create_block_widget as BxNativeFunction,
    );
    map.insert(
        "tui.Input".to_string(),
        create_input_widget as BxNativeFunction,
    );
    map.insert(
        "tui.Custom".to_string(),
        create_custom_widget as BxNativeFunction,
    );
    map.insert(
        "tui.VBox".to_string(),
        create_vbox_widget as BxNativeFunction,
    );
    map.insert(
        "tui.HBox".to_string(),
        create_hbox_widget as BxNativeFunction,
    );
    map.insert(
        "tui.Button".to_string(),
        create_button_widget as BxNativeFunction,
    );
    map.insert(
        "tui.ProgressBar".to_string(),
        create_progress_bar_widget as BxNativeFunction,
    );
    map
}
