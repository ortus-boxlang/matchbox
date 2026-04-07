use ratatui::layout::Rect;
use ratatui::Frame;
use std::cell::RefCell;
use std::collections::HashMap;

use matchbox_vm::{BxObject, bx_methods};
use matchbox_vm::types::{BxVM, BxValue, Tracer};
use crate::terminal::TUI;

#[derive(Clone, Debug)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug)]
pub enum BorderType {
    Plain,
    Rounded,
    Double,
    Thick,
}

#[derive(Clone, Debug)]
pub enum ListStyle {
    Plain,
    Bulleted,
    Numbered,
}

#[derive(Clone, Debug, BxObject)]
pub struct TextWidget {
    pub text: String,
    pub alignment: TextAlignment,
    pub wrap: bool,
    pub fg_color: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub z_index: i32,
}

#[bx_methods]
impl TextWidget {
    pub fn text(&mut self, text: String) -> &mut Self {
        self.text = text;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    pub fn color(&mut self, color: String) -> &mut Self {
        self.fg_color = Some(color);
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    pub fn align(&mut self, align: String) -> &mut Self {
        self.alignment = match align.to_lowercase().as_str() {
            "center" => TextAlignment::Center,
            "right" => TextAlignment::Right,
            _ => TextAlignment::Left,
        };
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    pub fn bold(&mut self, bold: bool) -> &mut Self {
        self.bold = bold;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn zIndex(&mut self, z: i32) -> &mut Self {
        self.z_index = z;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn __render(&self, vm: &mut dyn BxVM, ctx: BxValue, _area: BxValue) -> Result<(), String> {
        if let Some(ctx_id) = ctx.as_gc_id() {
            if self.z_index != 0 {
                let _ = vm.native_object_call_method(ctx_id, "setZIndex", &[BxValue::new_number(self.z_index as f64)]);
            }
            let text_id = vm.string_new(self.text.clone());
            vm.native_object_call_method(ctx_id, "drawText", &[
                BxValue::new_number(0.0),
                BxValue::new_number(0.0),
                BxValue::new_ptr(text_id),
            ])?;
        }
        Ok(())
    }

    pub fn build(&self) -> f64 {
        let widget = WidgetKind::Text(self.clone());
        WidgetRegistry::insert(widget) as f64
    }
}

#[derive(Clone, Debug, BxObject)]
pub struct ButtonWidget {
    pub label: String,
    pub on_click: Option<BxValue>,
    pub z_index: i32,
}

#[bx_methods]
impl ButtonWidget {
    pub fn label(&mut self, label: String) -> &mut Self {
        self.label = label;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn onClick(&mut self, callback: BxValue) -> &mut Self {
        self.on_click = Some(callback);
        self
    }

    #[allow(non_snake_case)]
    pub fn zIndex(&mut self, z: i32) -> &mut Self {
        self.z_index = z;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn __render(&self, vm: &mut dyn BxVM, ctx: BxValue, _area: BxValue) -> Result<(), String> {
        if let Some(ctx_id) = ctx.as_gc_id() {
            if self.z_index != 0 {
                let _ = vm.native_object_call_method(ctx_id, "setZIndex", &[BxValue::new_number(self.z_index as f64)]);
            }
            let label_id = vm.string_new(format!("[ {} ]", self.label));
            vm.native_object_call_method(ctx_id, "drawText", &[
                BxValue::new_number(0.0),
                BxValue::new_number(0.0),
                BxValue::new_ptr(label_id),
            ])?;
        }
        Ok(())
    }

    pub fn build(&self) -> f64 {
        let widget = WidgetKind::Button(self.clone());
        WidgetRegistry::insert(widget) as f64
    }
}

#[derive(Clone, Debug, BxObject)]
pub struct ListWidget {
    pub items: Vec<String>,
    pub selected: usize,
    pub style: ListStyle,
    pub highlight_symbol: Option<String>,
    pub z_index: i32,
}

#[bx_methods]
impl ListWidget {
    #[allow(non_snake_case)]
    pub fn addItem(&mut self, item: String) -> &mut Self {
        self.items.push(item);
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    pub fn clear(&mut self) -> &mut Self {
        self.items.clear();
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    pub fn selected(&mut self, index: i32) -> &mut Self {
        self.selected = index as usize;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    pub fn style(&mut self, style: String) -> &mut Self {
        self.style = match style.to_lowercase().as_str() {
            "bulleted" => ListStyle::Bulleted,
            "numbered" => ListStyle::Numbered,
            _ => ListStyle::Plain,
        };
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn zIndex(&mut self, z: i32) -> &mut Self {
        self.z_index = z;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn __render(&self, vm: &mut dyn BxVM, ctx: BxValue, area: BxValue) -> Result<(), String> {
        if let Some(ctx_id) = ctx.as_gc_id() {
            if self.z_index != 0 {
                let _ = vm.native_object_call_method(ctx_id, "setZIndex", &[BxValue::new_number(self.z_index as f64)]);
            }
            
            let area_id = area.as_gc_id().ok_or("Invalid area")?;
            let h = vm.struct_get(area_id, "h").as_number() as usize;
            
            for i in 0..self.items.len().min(h) {
                let prefix = if i == self.selected { "> " } else { "  " };
                let text = match self.style {
                    ListStyle::Plain => format!("{}{}", prefix, self.items[i]),
                    ListStyle::Bulleted => format!("{}• {}", prefix, self.items[i]),
                    ListStyle::Numbered => format!("{}{}. {}", prefix, i + 1, self.items[i]),
                };
                
                let text_id = vm.string_new(text);
                vm.native_object_call_method(ctx_id, "drawText", &[
                    BxValue::new_number(0.0),
                    BxValue::new_number(i as f64),
                    BxValue::new_ptr(text_id),
                ])?;
            }
        }
        Ok(())
    }

    pub fn build(&self) -> f64 {
        let widget = WidgetKind::List(self.clone());
        WidgetRegistry::insert(widget) as f64
    }
}

#[derive(Clone, Debug, BxObject)]
pub struct ProgressBarWidget {
    pub completed: usize,
    pub total: usize,
    pub start_color: Option<String>,
    pub end_color: Option<String>,
    pub empty_color: Option<String>,
    pub show_label: bool,
    pub label_position: String,
    pub fill_char: Option<String>,
    pub empty_char: Option<String>,
    pub z_index: i32,
}

#[bx_methods]
impl ProgressBarWidget {
    pub fn completed(&mut self, count: i32) -> &mut Self {
        self.completed = count as usize;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    pub fn total(&mut self, count: i32) -> &mut Self {
        self.total = count as usize;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn zIndex(&mut self, z: i32) -> &mut Self {
        self.z_index = z;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn __render(&self, vm: &mut dyn BxVM, ctx: BxValue, area: BxValue) -> Result<(), String> {
        if let Some(ctx_id) = ctx.as_gc_id() {
            if self.z_index != 0 {
                let _ = vm.native_object_call_method(ctx_id, "setZIndex", &[BxValue::new_number(self.z_index as f64)]);
            }
            
            let area_id = area.as_gc_id().ok_or("Invalid area")?;
            let w = vm.struct_get(area_id, "w").as_number();
            
            let pct = if self.total > 0 {
                (self.completed as f64 / self.total as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            
            let filled_w = (w * pct) as usize;
            let mut bar = String::new();
            for _ in 0..filled_w { bar.push('█'); }
            for _ in filled_w..(w as usize) { bar.push('░'); }
            
            let text_id = vm.string_new(bar);
            vm.native_object_call_method(ctx_id, "drawText", &[
                BxValue::new_number(0.0),
                BxValue::new_number(0.0),
                BxValue::new_ptr(text_id),
            ])?;
        }
        Ok(())
    }

    pub fn build(&self) -> f64 {
        let widget = WidgetKind::ProgressBar(self.clone());
        WidgetRegistry::insert(widget) as f64
    }
}

#[derive(Clone, Debug, BxObject)]
pub struct BlockWidget {
    pub title: String,
    pub border_type: BorderType,
    pub inner_widget: Option<BxValue>,
    pub z_index: i32,
}

#[bx_methods]
impl BlockWidget {
    pub fn title(&mut self, title: String) -> &mut Self {
        self.title = title;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    pub fn border(&mut self, border: String) -> &mut Self {
        self.border_type = match border.to_lowercase().as_str() {
            "rounded" => BorderType::Rounded,
            "double" => BorderType::Double,
            "thick" => BorderType::Thick,
            _ => BorderType::Plain,
        };
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn setWidget(&mut self, widget: BxValue) -> &mut Self {
        self.inner_widget = Some(widget);
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn zIndex(&mut self, z: i32) -> &mut Self {
        self.z_index = z;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn __render(&self, vm: &mut dyn BxVM, ctx: BxValue, area: BxValue) -> Result<(), String> {
        if let Some(ctx_id) = ctx.as_gc_id() {
            if self.z_index != 0 {
                let _ = vm.native_object_call_method(ctx_id, "setZIndex", &[BxValue::new_number(self.z_index as f64)]);
            }
            
            let area_id = area.as_gc_id().ok_or("Invalid area")?;
            let w = vm.struct_get(area_id, "w").as_number();
            let h = vm.struct_get(area_id, "h").as_number();
            
            // 1. Draw border
            vm.native_object_call_method(ctx_id, "drawRect", &[
                BxValue::new_number(0.0),
                BxValue::new_number(0.0),
                BxValue::new_number(w),
                BxValue::new_number(h),
            ])?;
            
            // 2. Draw title
            if !self.title.is_empty() {
                let title_text = format!(" {} ", self.title);
                let title_id = vm.string_new(title_text);
                vm.native_object_call_method(ctx_id, "drawText", &[
                    BxValue::new_number(2.0),
                    BxValue::new_number(0.0),
                    BxValue::new_ptr(title_id),
                ])?;
            }
            
            // 3. Inner widget via double dispatch
            if let Some(inner) = self.inner_widget {
                if let Some(inner_obj_id) = inner.as_gc_id() {
                    let inner_area_id = vm.struct_new();
                    vm.struct_set(inner_area_id, "x", BxValue::new_number(0.0));
                    vm.struct_set(inner_area_id, "y", BxValue::new_number(0.0));
                    vm.struct_set(inner_area_id, "w", BxValue::new_number((w - 2.0).max(0.0)));
                    vm.struct_set(inner_area_id, "h", BxValue::new_number((h - 2.0).max(0.0)));

                    // Root the temporary area struct
                    vm.push_root(BxValue::new_ptr(inner_area_id));

                    vm.native_object_call_method(ctx_id, "pushOrigin", &[
                        BxValue::new_number(1.0),
                        BxValue::new_number(1.0),
                    ])?;

                    let _ = vm.native_object_call_method(inner_obj_id, "__render", &[ctx, BxValue::new_ptr(inner_area_id)]);

                    vm.native_object_call_method(ctx_id, "popOrigin", &[])?;
                    
                    vm.pop_root();
                }
            }
        }
        Ok(())
    }

    pub fn build(&self) -> f64 {
        let widget = WidgetKind::Block(self.clone());
        WidgetRegistry::insert(widget) as f64
    }
}

#[derive(Clone, Debug, BxObject)]
pub struct InputWidget {
    pub value: String,
    pub placeholder: String,
    pub prompt: String,
    pub z_index: i32,
}

#[bx_methods]
impl InputWidget {
    pub fn value(&mut self, value: String) -> &mut Self {
        self.value = value;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    pub fn placeholder(&mut self, placeholder: String) -> &mut Self {
        self.placeholder = placeholder;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    pub fn prompt(&mut self, prompt: String) -> &mut Self {
        self.prompt = prompt;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn zIndex(&mut self, z: i32) -> &mut Self {
        self.z_index = z;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn __render(&self, vm: &mut dyn BxVM, ctx: BxValue, area: BxValue) -> Result<(), String> {
        if let Some(ctx_id) = ctx.as_gc_id() {
            if self.z_index != 0 {
                let _ = vm.native_object_call_method(ctx_id, "setZIndex", &[BxValue::new_number(self.z_index as f64)]);
            }
            
            let area_id = area.as_gc_id().ok_or("Invalid area")?;
            let w = vm.struct_get(area_id, "w").as_number();
            let h = vm.struct_get(area_id, "h").as_number();
            
            // Draw a rectangle for the input box
            vm.native_object_call_method(ctx_id, "drawRect", &[
                BxValue::new_number(0.0),
                BxValue::new_number(0.0),
                BxValue::new_number(w),
                BxValue::new_number(h),
            ])?;
            
            // Draw prompt + value
            let display_text = if self.value.is_empty() {
                format!("{} {}", self.prompt, self.placeholder)
            } else {
                format!("{} {}", self.prompt, self.value)
            };
            
            let text_id = vm.string_new(display_text);
            vm.native_object_call_method(ctx_id, "drawText", &[
                BxValue::new_number(1.0), // Padding inside border
                BxValue::new_number(1.0),
                BxValue::new_ptr(text_id),
            ])?;
        }
        Ok(())
    }

    pub fn build(&self) -> f64 {
        let widget = WidgetKind::Input(self.clone());
        WidgetRegistry::insert(widget) as f64
    }
}

#[derive(Clone, Debug, BxObject)]
pub struct VBoxWidget {
    pub children: Vec<BxValue>,
    pub z_index: i32,
}

#[bx_methods]
impl VBoxWidget {
    pub fn add(&mut self, child: BxValue) -> &mut Self {
        self.children.push(child);
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn zIndex(&mut self, z: i32) -> &mut Self {
        self.z_index = z;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn __render(&self, vm: &mut dyn BxVM, ctx: BxValue, area: BxValue) -> Result<(), String> {
        if self.children.is_empty() { return Ok(()); }
        
        let area_id = area.as_gc_id().ok_or("Invalid area")?;
        let w = vm.struct_get(area_id, "w").as_number();
        let h = vm.struct_get(area_id, "h").as_number();
        
        let child_h = h / self.children.len() as f64;
        
        for (i, child) in self.children.iter().enumerate() {
            let child_y_offset = i as f64 * child_h;
            
            let child_area_id = vm.struct_new();
            vm.struct_set(child_area_id, "x", BxValue::new_number(0.0));
            vm.struct_set(child_area_id, "y", BxValue::new_number(0.0));
            vm.struct_set(child_area_id, "w", BxValue::new_number(w));
            vm.struct_set(child_area_id, "h", BxValue::new_number(child_h));
            
            // Root temporary area
            vm.push_root(BxValue::new_ptr(child_area_id));

            if let Some(child_obj_id) = child.as_gc_id() {
                if let Some(ctx_id) = ctx.as_gc_id() {
                    vm.native_object_call_method(ctx_id, "pushOrigin", &[
                        BxValue::new_number(0.0),
                        BxValue::new_number(child_y_offset),
                    ])?;
                }

                let _ = vm.native_object_call_method(child_obj_id, "__render", &[ctx, BxValue::new_ptr(child_area_id)]);

                if let Some(ctx_id) = ctx.as_gc_id() {
                    let _ = vm.native_object_call_method(ctx_id, "popOrigin", &[]);
                }
            }
            
            vm.pop_root();
        }
        Ok(())
    }

    pub fn build(&self) -> f64 {
        let widget = WidgetKind::VBox(self.clone());
        WidgetRegistry::insert(widget) as f64
    }
}

#[derive(Clone, Debug, BxObject)]
pub struct HBoxWidget {
    pub children: Vec<BxValue>,
    pub z_index: i32,
}

#[bx_methods]
impl HBoxWidget {
    pub fn add(&mut self, child: BxValue) -> &mut Self {
        self.children.push(child);
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn zIndex(&mut self, z: i32) -> &mut Self {
        self.z_index = z;
        TUI::with_current(|tui| tui.set_dirty());
        self
    }

    #[allow(non_snake_case)]
    pub fn __render(&self, vm: &mut dyn BxVM, ctx: BxValue, area: BxValue) -> Result<(), String> {
        if self.children.is_empty() { return Ok(()); }
        
        let area_id = area.as_gc_id().ok_or("Invalid area")?;
        let w = vm.struct_get(area_id, "w").as_number();
        let h = vm.struct_get(area_id, "h").as_number();
        
        let child_w = w / self.children.len() as f64;
        
        for (i, child) in self.children.iter().enumerate() {
            let child_x_offset = i as f64 * child_w;
            
            let child_area_id = vm.struct_new();
            vm.struct_set(child_area_id, "x", BxValue::new_number(0.0));
            vm.struct_set(child_area_id, "y", BxValue::new_number(0.0));
            vm.struct_set(child_area_id, "w", BxValue::new_number(child_w));
            vm.struct_set(child_area_id, "h", BxValue::new_number(h));
            
            // Root temporary area
            vm.push_root(BxValue::new_ptr(child_area_id));

            if let Some(child_obj_id) = child.as_gc_id() {
                if let Some(ctx_id) = ctx.as_gc_id() {
                    vm.native_object_call_method(ctx_id, "pushOrigin", &[
                        BxValue::new_number(child_x_offset),
                        BxValue::new_number(0.0),
                    ])?;
                }

                let _ = vm.native_object_call_method(child_obj_id, "__render", &[ctx, BxValue::new_ptr(child_area_id)]);

                if let Some(ctx_id) = ctx.as_gc_id() {
                    let _ = vm.native_object_call_method(ctx_id, "popOrigin", &[]);
                }
            }
            
            vm.pop_root();
        }
        Ok(())
    }

    pub fn build(&self) -> f64 {
        let widget = WidgetKind::HBox(self.clone());
        WidgetRegistry::insert(widget) as f64
    }
}

#[derive(Clone, Debug)]
pub struct TableColumn {
    pub name: String,
    pub width: Option<u16>,
}

#[derive(Clone, Debug, BxObject)]
pub struct TableWidget {
    pub columns: Vec<TableColumn>,
    pub rows: Vec<Vec<String>>,
    pub selected: usize,
    pub show_header: bool,
    pub column_widths: Option<Vec<u16>>,
}

#[bx_methods]
impl TableWidget {
    pub fn build(&self) -> f64 {
        let widget = WidgetKind::Table(self.clone());
        WidgetRegistry::insert(widget) as f64
    }
}

#[derive(Clone, Debug)]
pub enum WidgetKind {
    Text(TextWidget),
    List(ListWidget),
    Table(TableWidget),
    Block(BlockWidget),
    Input(InputWidget),
    ProgressBar(ProgressBarWidget),
    Custom(BxValue),
    VBox(VBoxWidget),
    HBox(HBoxWidget),
    Button(ButtonWidget),
}

impl WidgetKind {
    pub fn render_in_area(&self, vm: &mut dyn BxVM, frame: &mut Frame, area: Rect, _widget_registry: &WidgetRegistry) {
        match self {
            WidgetKind::Text(text) => text.render_in_area(frame, area),
            WidgetKind::List(list) => { let _ = self.render_with_context(vm, frame, area, list.z_index, |vm, ctx, a| list.__render(vm, ctx, a)); }
            WidgetKind::Table(table) => table.render_in_area(frame, area),
            WidgetKind::Block(block) => { let _ = self.render_with_context(vm, frame, area, block.z_index, |vm, ctx, a| block.__render(vm, ctx, a)); }
            WidgetKind::Input(input) => { let _ = self.render_with_context(vm, frame, area, input.z_index, |vm, ctx, a| input.__render(vm, ctx, a)); }
            WidgetKind::ProgressBar(bar) => { let _ = self.render_with_context(vm, frame, area, bar.z_index, |vm, ctx, a| bar.__render(vm, ctx, a)); }
            WidgetKind::Button(button) => button.render_in_area(frame, area),
            WidgetKind::Custom(obj) => {
                let _ = self.render_with_double_dispatch(vm, *obj, frame, area);
            }
            WidgetKind::VBox(vbox) => {
                let _ = self.render_vbox(vm, vbox, frame, area);
            }
            WidgetKind::HBox(hbox) => {
                let _ = self.render_hbox(vm, hbox, frame, area);
            }
        }
    }

    pub fn render_to_context(&self, vm: &mut dyn BxVM, ctx: BxValue, area: BxValue) -> Result<(), String> {
        match self {
            WidgetKind::Text(text) => text.__render(vm, ctx, area),
            WidgetKind::List(list) => list.__render(vm, ctx, area),
            WidgetKind::Table(_) => Ok(()), 
            WidgetKind::Block(block) => block.__render(vm, ctx, area),
            WidgetKind::Input(input) => input.__render(vm, ctx, area),
            WidgetKind::ProgressBar(bar) => bar.__render(vm, ctx, area),
            WidgetKind::Button(button) => button.__render(vm, ctx, area),
            WidgetKind::Custom(obj) => {
                if let Some(obj_id) = obj.as_gc_id() {
                    vm.native_object_call_method(obj_id, "__render", &[ctx, area])?;
                }
                Ok(())
            }
            WidgetKind::VBox(vbox) => vbox.__render(vm, ctx, area),
            WidgetKind::HBox(hbox) => hbox.__render(vm, ctx, area),
        }
    }

    pub fn trace(&self, tracer: &mut dyn Tracer) {
        use matchbox_vm::types::BxNativeObject;
        match self {
            WidgetKind::Text(w) => w.trace(tracer),
            WidgetKind::List(w) => w.trace(tracer),
            WidgetKind::Block(w) => w.trace(tracer),
            WidgetKind::Input(w) => w.trace(tracer),
            WidgetKind::ProgressBar(w) => w.trace(tracer),
            WidgetKind::VBox(w) => w.trace(tracer),
            WidgetKind::HBox(w) => w.trace(tracer),
            WidgetKind::Button(w) => w.trace(tracer),
            WidgetKind::Custom(val) => tracer.mark(val),
            WidgetKind::Table(w) => w.trace(tracer),
        }
    }

    fn render_with_context(&self, vm: &mut dyn BxVM, frame: &mut Frame, area: Rect, z_index: i32, f: impl FnOnce(&mut dyn BxVM, BxValue, BxValue) -> Result<(), String>) -> Result<(), String> {
        use crate::rendering_context::RenderingContext;
        use std::rc::Rc;
        let mut ctx = RenderingContext::new();
        ctx.current_origin = (area.x, area.y);
        ctx.current_z_index = z_index;
        
        let ctx_rc = Rc::new(RefCell::new(ctx));
        let ctx_obj_id = vm.native_object_new(ctx_rc.clone());
        let area_id = self.create_area_struct(vm, area);
        
        // Root temporary context and area
        vm.push_root(BxValue::new_ptr(ctx_obj_id));
        vm.push_root(BxValue::new_ptr(area_id));

        let res = f(vm, BxValue::new_ptr(ctx_obj_id), BxValue::new_ptr(area_id));
        
        if res.is_ok() {
            ctx_rc.borrow_mut().playback(frame);
        }
        
        vm.pop_root(); // Pop area_id
        vm.pop_root(); // Pop ctx_obj_id
        
        res
    }

    fn render_vbox(&self, vm: &mut dyn BxVM, vbox: &VBoxWidget, frame: &mut Frame, area: Rect) -> Result<(), String> {
        self.render_with_context(vm, frame, area, vbox.z_index, |vm, ctx, a| vbox.__render(vm, ctx, a))
    }

    fn render_hbox(&self, vm: &mut dyn BxVM, hbox: &HBoxWidget, frame: &mut Frame, area: Rect) -> Result<(), String> {
        self.render_with_context(vm, frame, area, hbox.z_index, |vm, ctx, a| hbox.__render(vm, ctx, a))
    }

    fn render_with_double_dispatch(&self, vm: &mut dyn BxVM, obj: BxValue, frame: &mut Frame, area: Rect) -> Result<(), String> {
        self.render_with_context(vm, frame, area, 0, |vm, ctx, a| {
            if let Some(obj_id) = obj.as_gc_id() {
                vm.native_object_call_method(obj_id, "__render", &[ctx, a])?;
            }
            Ok(())
        })
    }

    fn create_area_struct(&self, vm: &mut dyn BxVM, area: Rect) -> usize {
        let area_id = vm.struct_new();
        vm.struct_set(area_id, "x", BxValue::new_number(area.x as f64));
        vm.struct_set(area_id, "y", BxValue::new_number(area.y as f64));
        vm.struct_set(area_id, "w", BxValue::new_number(area.width as f64));
        vm.struct_set(area_id, "h", BxValue::new_number(area.height as f64));
        area_id
    }
}

pub trait RenderInArea {
    fn render_in_area(&self, frame: &mut Frame, area: Rect);
}

impl RenderInArea for TextWidget {
    fn render_in_area(&self, frame: &mut Frame, area: Rect) {
        use ratatui::layout::Alignment;
        use ratatui::style::{Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Paragraph, Widget};

        let mut style = Style::default();
        if let Some(ref color) = self.fg_color {
            style = style.fg(parse_color(color));
        }
        if self.bold {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }

        let span = Span::styled(&self.text, style);
        let line = Line::from(span);
        let mut paragraph = Paragraph::new(line);

        match self.alignment {
            TextAlignment::Left => paragraph = paragraph.alignment(Alignment::Left),
            TextAlignment::Center => paragraph = paragraph.alignment(Alignment::Center),
            TextAlignment::Right => paragraph = paragraph.alignment(Alignment::Right),
        }

        if self.wrap {
            paragraph = paragraph.wrap(ratatui::widgets::Wrap { trim: true });
        }

        paragraph.render(area, frame.buffer_mut());
    }
}

impl RenderInArea for ButtonWidget {
    fn render_in_area(&self, frame: &mut Frame, area: Rect) {
        use ratatui::widgets::{Paragraph, Widget, Block, Borders};
        use ratatui::style::{Style, Modifier};
        
        let block = Block::default().borders(Borders::ALL);
        let p = Paragraph::new(self.label.as_str())
            .block(block)
            .style(Style::default().add_modifier(Modifier::BOLD));
        p.render(area, frame.buffer_mut());
    }
}

impl RenderInArea for TableWidget {
    fn render_in_area(&self, frame: &mut Frame, area: Rect) {
        use ratatui::style::{Modifier, Style};
        use ratatui::widgets::{Cell, Row, StatefulWidget, Table, TableState};

        let header_row = if self.show_header {
            let cells: Vec<Cell> = self
                .columns
                .iter()
                .map(|col| Cell::from(col.name.clone()))
                .collect();
            Some(Row::new(cells).style(Style::default().add_modifier(Modifier::BOLD)))
        } else {
            None
        };

        let rows: Vec<Row> = self
            .rows
            .iter()
            .map(|row| {
                let cells: Vec<Cell> = row.iter().map(|val| Cell::from(val.clone())).collect();
                Row::new(cells)
            })
            .collect();

        let mut table = Table::new(rows, self.get_constraints());
        if let Some(header) = header_row {
            table = table.header(header);
        }
        table = table.block(ratatui::widgets::Block::default());
        table = table.row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = TableState::default();
        state.select(Some(self.selected));

        StatefulWidget::render(table, area, frame.buffer_mut(), &mut state);
    }
}

impl TableWidget {
    fn get_constraints(&self) -> Vec<ratatui::layout::Constraint> {
        use ratatui::layout::Constraint;
        if let Some(ref widths) = self.column_widths {
            widths.iter().map(|w| Constraint::Length(*w)).collect()
        } else if !self.columns.is_empty() {
            let n = self.columns.len();
            vec![Constraint::Ratio(1, n as u32); n]
        } else {
            vec![Constraint::Percentage(100)]
        }
    }
}

impl RenderInAreaWithRegistry for BlockWidget {
    fn render_in_area(&self, vm: &mut dyn BxVM, frame: &mut Frame, area: Rect, _widget_registry: &WidgetRegistry) {
        use ratatui::widgets::{Block, BorderType as RatatuiBorderType, Widget};

        let mut block = Block::bordered();
        if !self.title.is_empty() {
            block = block.title(self.title.clone());
        }

        match self.border_type {
            BorderType::Plain => block = block.border_type(RatatuiBorderType::Plain),
            BorderType::Rounded => block = block.border_type(RatatuiBorderType::Rounded),
            BorderType::Double => block = block.border_type(RatatuiBorderType::Double),
            BorderType::Thick => block = block.border_type(RatatuiBorderType::Thick),
        }

        if let Some(inner) = self.inner_widget {
            if let Some(inner_id) = inner.as_gc_id() {
                let inner_area = block.inner(area);
                block.render(area, frame.buffer_mut());
                
                if let Some(widget) = WidgetRegistry::get(inner_id) {
                    widget.render_in_area(vm, frame, inner_area, _widget_registry);
                }
                return;
            }
        }

        block.render(area, frame.buffer_mut());
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
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        _ => Color::White,
    }
}

thread_local! {
    static WIDGET_REGISTRY: RefCell<WidgetRegistryInner> = RefCell::new(WidgetRegistryInner::new());
}

struct WidgetRegistryInner {
    widgets: HashMap<usize, WidgetKind>,
    next_id: usize,
}

impl WidgetRegistryInner {
    fn new() -> Self {
        Self {
            widgets: HashMap::new(),
            next_id: 1,
        }
    }
}

pub struct WidgetRegistry;

impl WidgetRegistry {
    pub fn insert(widget: WidgetKind) -> usize {
        WIDGET_REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            let id = r.next_id;
            r.next_id += 1;
            r.widgets.insert(id, widget);
            id
        })
    }

    pub fn get(id: usize) -> Option<WidgetKind> {
        WIDGET_REGISTRY.with(|r| r.borrow().widgets.get(&id).cloned())
    }

    pub fn remove(id: usize) -> Option<WidgetKind> {
        WIDGET_REGISTRY.with(|r| r.borrow_mut().widgets.remove(&id))
    }

    pub fn clear() {
        WIDGET_REGISTRY.with(|r| {
            let mut r = r.borrow_mut();
            r.widgets.clear();
            // NEVER reset next_id to prevent overlapping IDs in current frame
        });
    }

    pub fn trace(tracer: &mut dyn Tracer) {
        WIDGET_REGISTRY.with(|r| {
            for widget in r.borrow().widgets.values() {
                widget.trace(tracer);
            }
        });
    }
}

pub trait RenderInAreaWithRegistry {
    fn render_in_area(&self, vm: &mut dyn BxVM, frame: &mut Frame, area: Rect, widget_registry: &WidgetRegistry);
}
