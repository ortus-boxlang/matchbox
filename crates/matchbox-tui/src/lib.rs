use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use matchbox_vm::types::{BxNativeFunction, BxNativeObject, BxVM, BxValue};

mod terminal;
mod widget;

pub use terminal::TUI;
pub use widget::{
    BlockWidget, BorderType, InputWidget, ListStyle, ListWidget, ProgressBarWidget, TableColumn,
    TableWidget, TextAlignment, TextWidget, WidgetKind, WidgetRegistry,
};

impl BxNativeObject for TUI {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "init" => {
                self.init()?;
                Ok(BxValue::new_null())
            }
            "beginframe" => {
                self.begin_frame();
                Ok(BxValue::new_null())
            }
            "endframe" => {
                self.end_frame()?;
                Ok(BxValue::new_null())
            }
            "print" => {
                if args.len() < 3 {
                    return Err("print requires at least 3 arguments: (x, y, text)".to_string());
                }
                let x = args[0].as_number() as u16;
                let y = args[1].as_number() as u16;
                let text = vm.to_string(args[2]);
                let color = if args.len() > 3 {
                    vm.to_string(args[3])
                } else {
                    "white".to_string()
                };
                let bold = if args.len() > 4 {
                    args[4].as_bool()
                } else {
                    false
                };
                self.print(x, y, &text, &color, bold)?;
                Ok(BxValue::new_null())
            }
            "renderwidget" => {
                if args.len() != 5 {
                    return Err(
                        "renderWidget requires 5 arguments: (widgetId, x, y, width, height)"
                            .to_string(),
                    );
                }
                let widget_id = args[0].as_number() as usize;
                let x = args[1].as_number() as u16;
                let y = args[2].as_number() as u16;
                let width = args[3].as_number() as u16;
                let height = args[4].as_number() as u16;
                self.render_widget(widget_id, x, y, width, height);
                Ok(BxValue::new_null())
            }
            "getkey" => {
                let key = self.get_key()?;
                Ok(BxValue::new_ptr(vm.string_new(key)))
            }
            "pollkey" => {
                if args.is_empty() {
                    return Err("pollKey requires 1 argument: (timeoutMs)".to_string());
                }
                let timeout = args[0].as_number() as u64;
                let key = self.poll_key(timeout)?;
                Ok(BxValue::new_ptr(vm.string_new(key)))
            }
            "size" => {
                let (w, h) = self.size()?;
                let s = vm.struct_new();
                vm.struct_set(s, "width", BxValue::new_number(w as f64));
                vm.struct_set(s, "height", BxValue::new_number(h as f64));
                Ok(BxValue::new_ptr(s))
            }
            "clear" => {
                self.clear()?;
                Ok(BxValue::new_null())
            }
            "setmouse" => {
                if args.is_empty() {
                    return Err("setMouse requires 1 argument: (enabled)".to_string());
                }
                let enabled = args[0].as_bool();
                self.set_mouse(enabled)?;
                Ok(BxValue::new_null())
            }
            "shutdown" => {
                self.shutdown()?;
                Ok(BxValue::new_null())
            }
            _ => Err(format!("Method {} not found", name)),
        }
    }
}

#[derive(Debug)]
pub struct TextWidgetNative {
    pub widget: TextWidget,
}

impl BxNativeObject for TextWidgetNative {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "settext" => {
                if args.is_empty() {
                    return Err("setText requires 1 argument: (text)".to_string());
                }
                self.widget.text = vm.to_string(args[0]);
                Ok(BxValue::new_null())
            }
            "setalignment" => {
                if args.is_empty() {
                    return Err("setAlignment requires 1 argument: (alignment)".to_string());
                }
                let align = vm.to_string(args[0]).to_lowercase();
                self.widget.alignment = match align.as_str() {
                    "center" => TextAlignment::Center,
                    "right" => TextAlignment::Right,
                    _ => TextAlignment::Left,
                };
                Ok(BxValue::new_null())
            }
            "setwrap" => {
                if args.is_empty() {
                    return Err("setWrap requires 1 argument: (wrap)".to_string());
                }
                self.widget.wrap = args[0].as_bool();
                Ok(BxValue::new_null())
            }
            "setcolor" => {
                if args.is_empty() {
                    return Err("setColor requires 1 argument: (color)".to_string());
                }
                self.widget.fg_color = Some(vm.to_string(args[0]));
                Ok(BxValue::new_null())
            }
            "setbold" => {
                if args.is_empty() {
                    return Err("setBold requires 1 argument: (bold)".to_string());
                }
                self.widget.bold = args[0].as_bool();
                Ok(BxValue::new_null())
            }
            "build" => {
                let widget = WidgetKind::Text(self.widget.clone());
                let id = WidgetRegistry::with_current(|r| r.insert(widget));
                Ok(BxValue::new_number(id as f64))
            }
            _ => Err(format!("Method {} not found", name)),
        }
    }
}

#[derive(Debug)]
pub struct ListWidgetNative {
    pub widget: ListWidget,
}

impl BxNativeObject for ListWidgetNative {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "additem" => {
                if args.is_empty() {
                    return Err("addItem requires 1 argument: (text)".to_string());
                }
                self.widget.items.push(vm.to_string(args[0]));
                Ok(BxValue::new_null())
            }
            "additems" => {
                if args.is_empty() {
                    return Err("addItems requires 1 argument: (array)".to_string());
                }
                if let Some(id) = args[0].as_gc_id() {
                    let len = vm.get_len(id);
                    for i in 0..len {
                        let val = vm.array_get(id, i);
                        self.widget.items.push(vm.to_string(val));
                    }
                }
                Ok(BxValue::new_null())
            }
            "setselected" => {
                if args.is_empty() {
                    return Err("setSelected requires 1 argument: (index)".to_string());
                }
                self.widget.selected = args[0].as_number() as usize;
                Ok(BxValue::new_null())
            }
            "getselected" => Ok(BxValue::new_number(self.widget.selected as f64)),
            "setstyle" => {
                if args.is_empty() {
                    return Err("setStyle requires 1 argument: (style)".to_string());
                }
                let style = vm.to_string(args[0]).to_lowercase();
                self.widget.style = match style.as_str() {
                    "bulleted" => ListStyle::Bulleted,
                    "numbered" => ListStyle::Numbered,
                    _ => ListStyle::Plain,
                };
                Ok(BxValue::new_null())
            }
            "sethighlightsymbol" => {
                if args.is_empty() {
                    return Err("setHighlightSymbol requires 1 argument: (symbol)".to_string());
                }
                self.widget.highlight_symbol = Some(vm.to_string(args[0]));
                Ok(BxValue::new_null())
            }
            "build" => {
                let widget = WidgetKind::List(self.widget.clone());
                let id = WidgetRegistry::with_current(|r| r.insert(widget));
                Ok(BxValue::new_number(id as f64))
            }
            _ => Err(format!("Method {} not found", name)),
        }
    }
}

#[derive(Debug)]
pub struct TableWidgetNative {
    pub widget: TableWidget,
}

impl BxNativeObject for TableWidgetNative {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "addcolumn" => {
                if args.is_empty() {
                    return Err("addColumn requires 1 argument: (name)".to_string());
                }
                let name = vm.to_string(args[0]);
                let width = if args.len() > 1 {
                    Some(args[1].as_number() as u16)
                } else {
                    None
                };
                self.widget
                    .columns
                    .push(widget::TableColumn { name, width });
                Ok(BxValue::new_null())
            }
            "addrow" => {
                if args.is_empty() {
                    return Err("addRow requires 1 argument: (array)".to_string());
                }
                if let Some(id) = args[0].as_gc_id() {
                    let len = vm.get_len(id);
                    let mut row = Vec::new();
                    for i in 0..len {
                        let val = vm.array_get(id, i);
                        row.push(vm.to_string(val));
                    }
                    self.widget.rows.push(row);
                }
                Ok(BxValue::new_null())
            }
            "setselected" => {
                if args.is_empty() {
                    return Err("setSelected requires 1 argument: (row)".to_string());
                }
                self.widget.selected = args[0].as_number() as usize;
                Ok(BxValue::new_null())
            }
            "getselected" => Ok(BxValue::new_number(self.widget.selected as f64)),
            "setheader" => {
                if args.is_empty() {
                    return Err("setHeader requires 1 argument: (show)".to_string());
                }
                self.widget.show_header = args[0].as_bool();
                Ok(BxValue::new_null())
            }
            "build" => {
                let widget = WidgetKind::Table(self.widget.clone());
                let id = WidgetRegistry::with_current(|r| r.insert(widget));
                Ok(BxValue::new_number(id as f64))
            }
            _ => Err(format!("Method {} not found", name)),
        }
    }
}

#[derive(Debug)]
pub struct BlockWidgetNative {
    pub widget: BlockWidget,
}

impl BxNativeObject for BlockWidgetNative {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "settitle" => {
                if args.is_empty() {
                    return Err("setTitle requires 1 argument: (title)".to_string());
                }
                self.widget.title = vm.to_string(args[0]);
                Ok(BxValue::new_null())
            }
            "setborder" => {
                if args.is_empty() {
                    return Err("setBorder requires 1 argument: (type)".to_string());
                }
                let border = vm.to_string(args[0]).to_lowercase();
                self.widget.border_type = match border.as_str() {
                    "rounded" => BorderType::Rounded,
                    "double" => BorderType::Double,
                    "thick" => BorderType::Thick,
                    _ => BorderType::Plain,
                };
                Ok(BxValue::new_null())
            }
            "setwidget" => {
                if args.is_empty() {
                    return Err("setWidget requires 1 argument: (widgetId)".to_string());
                }
                self.widget.inner_widget_id = Some(args[0].as_number() as usize);
                Ok(BxValue::new_null())
            }
            "build" => {
                let widget = WidgetKind::Block(self.widget.clone());
                let id = WidgetRegistry::with_current(|r| r.insert(widget));
                Ok(BxValue::new_number(id as f64))
            }
            _ => Err(format!("Method {} not found", name)),
        }
    }
}

#[derive(Debug)]
pub struct InputWidgetNative {
    pub widget: InputWidget,
}

impl BxNativeObject for InputWidgetNative {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "setprompt" => {
                if args.is_empty() {
                    return Err("setPrompt requires 1 argument: (prompt)".to_string());
                }
                self.widget.prompt = vm.to_string(args[0]);
                Ok(BxValue::new_null())
            }
            "setplaceholder" => {
                if args.is_empty() {
                    return Err("setPlaceholder requires 1 argument: (placeholder)".to_string());
                }
                self.widget.placeholder = vm.to_string(args[0]);
                Ok(BxValue::new_null())
            }
            "setvalue" => {
                if args.is_empty() {
                    return Err("setValue requires 1 argument: (value)".to_string());
                }
                self.widget.value = vm.to_string(args[0]);
                Ok(BxValue::new_null())
            }
            "getvalue" => Ok(BxValue::new_ptr(vm.string_new(self.widget.value.clone()))),
            "build" => {
                let widget = WidgetKind::Input(self.widget.clone());
                let id = WidgetRegistry::with_current(|r| r.insert(widget));
                Ok(BxValue::new_number(id as f64))
            }
            _ => Err(format!("Method {} not found", name)),
        }
    }
}

pub fn create_tui(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let tui = TUI::new();
    let id = vm.native_object_new(Rc::new(RefCell::new(tui)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_text_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = TextWidgetNative {
        widget: TextWidget {
            text: String::new(),
            alignment: TextAlignment::Left,
            wrap: false,
            fg_color: None,
            bold: false,
            italic: false,
            underline: false,
        },
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_list_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = ListWidgetNative {
        widget: ListWidget {
            items: Vec::new(),
            selected: 0,
            style: ListStyle::Plain,
            highlight_symbol: None,
        },
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_table_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = TableWidgetNative {
        widget: TableWidget {
            columns: Vec::new(),
            rows: Vec::new(),
            selected: 0,
            show_header: true,
            column_widths: None,
        },
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_block_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = BlockWidgetNative {
        widget: BlockWidget {
            title: String::new(),
            border_type: BorderType::Plain,
            inner_widget_id: None,
        },
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn create_input_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = InputWidgetNative {
        widget: InputWidget {
            value: String::new(),
            placeholder: String::new(),
            prompt: String::new(),
        },
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

#[derive(Debug)]
pub struct ProgressBarWidgetNative {
    pub widget: ProgressBarWidget,
}

impl BxNativeObject for ProgressBarWidgetNative {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "setcompleted" => {
                if args.is_empty() {
                    return Err("setCompleted requires 1 argument: (count)".to_string());
                }
                self.widget.completed = args[0].as_number() as usize;
                Ok(BxValue::new_null())
            }
            "settotal" => {
                if args.is_empty() {
                    return Err("setTotal requires 1 argument: (count)".to_string());
                }
                self.widget.total = args[0].as_number() as usize;
                Ok(BxValue::new_null())
            }
            "setstartcolor" => {
                if args.is_empty() {
                    return Err("setStartColor requires 1 argument: (color)".to_string());
                }
                self.widget.start_color = Some(vm.to_string(args[0]));
                Ok(BxValue::new_null())
            }
            "setendcolor" => {
                if args.is_empty() {
                    return Err("setEndColor requires 1 argument: (color)".to_string());
                }
                self.widget.end_color = Some(vm.to_string(args[0]));
                Ok(BxValue::new_null())
            }
            "setemptycolor" => {
                if args.is_empty() {
                    return Err("setEmptyColor requires 1 argument: (color)".to_string());
                }
                self.widget.empty_color = Some(vm.to_string(args[0]));
                Ok(BxValue::new_null())
            }
            "setshowlabel" => {
                if args.is_empty() {
                    return Err("setShowLabel requires 1 argument: (show)".to_string());
                }
                self.widget.show_label = args[0].as_bool();
                Ok(BxValue::new_null())
            }
            "setfillchar" => {
                if args.is_empty() {
                    return Err("setFillChar requires 1 argument: (char)".to_string());
                }
                self.widget.fill_char = Some(vm.to_string(args[0]));
                Ok(BxValue::new_null())
            }
            "setemptychar" => {
                if args.is_empty() {
                    return Err("setEmptyChar requires 1 argument: (char)".to_string());
                }
                self.widget.empty_char = Some(vm.to_string(args[0]));
                Ok(BxValue::new_null())
            }
            "build" => {
                let widget = WidgetKind::ProgressBar(self.widget.clone());
                let id = WidgetRegistry::with_current(|r| r.insert(widget));
                Ok(BxValue::new_number(id as f64))
            }
            _ => Err(format!("Method {} not found", name)),
        }
    }
}

pub fn create_progress_bar_widget(vm: &mut dyn BxVM, _args: &[BxValue]) -> Result<BxValue, String> {
    let widget = ProgressBarWidgetNative {
        widget: ProgressBarWidget {
            completed: 0,
            total: 0,
            start_color: None,
            end_color: None,
            empty_color: None,
            show_label: true,
            label_position: "center".to_string(),
            fill_char: None,
            empty_char: None,
        },
    };
    let id = vm.native_object_new(Rc::new(RefCell::new(widget)));
    Ok(BxValue::new_ptr(id))
}

pub fn register_classes() -> HashMap<String, BxNativeFunction> {
    let mut map = HashMap::new();
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
        "tui.ProgressBar".to_string(),
        create_progress_bar_widget as BxNativeFunction,
    );
    map
}
