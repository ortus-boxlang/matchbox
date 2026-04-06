use ratatui::layout::Rect;
use ratatui::Frame;
use std::cell::RefCell;
use std::collections::HashMap;

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

#[derive(Clone, Debug)]
pub struct TextWidget {
    pub text: String,
    pub alignment: TextAlignment,
    pub wrap: bool,
    pub fg_color: Option<String>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone, Debug)]
pub struct ListWidget {
    pub items: Vec<String>,
    pub selected: usize,
    pub style: ListStyle,
    pub highlight_symbol: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TableColumn {
    pub name: String,
    pub width: Option<u16>,
}

#[derive(Clone, Debug)]
pub struct TableWidget {
    pub columns: Vec<TableColumn>,
    pub rows: Vec<Vec<String>>,
    pub selected: usize,
    pub show_header: bool,
    pub column_widths: Option<Vec<u16>>,
}

#[derive(Clone, Debug)]
pub struct BlockWidget {
    pub title: String,
    pub border_type: BorderType,
    pub inner_widget_id: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct InputWidget {
    pub value: String,
    pub placeholder: String,
    pub prompt: String,
}

#[derive(Clone, Debug)]
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
}

use matchbox_vm::types::{BxVM, BxValue};

pub enum WidgetKind {
    Text(TextWidget),
    List(ListWidget),
    Table(TableWidget),
    Block(BlockWidget),
    Input(InputWidget),
    ProgressBar(ProgressBarWidget),
    Custom(BxValue),
}

impl WidgetKind {
    pub fn render_in_area(&self, vm: &mut dyn BxVM, frame: &mut Frame, area: Rect, widget_registry: &WidgetRegistry) {
        match self {
            WidgetKind::Text(text) => text.render_in_area(frame, area),
            WidgetKind::List(list) => list.render_in_area(frame, area),
            WidgetKind::Table(table) => table.render_in_area(frame, area),
            WidgetKind::Block(block) => block.render_in_area(vm, frame, area, widget_registry),
            WidgetKind::Input(input) => input.render_in_area(frame, area),
            WidgetKind::ProgressBar(bar) => bar.render_in_area(frame, area),
            WidgetKind::Custom(obj) => {
                use crate::rendering_context::RenderingContext;
                use std::rc::Rc;
                
                // 1. Create area struct in BoxLang
                let area_id = vm.struct_new();
                vm.struct_set(area_id, "x", BxValue::new_number(area.x as f64));
                vm.struct_set(area_id, "y", BxValue::new_number(area.y as f64));
                vm.struct_set(area_id, "w", BxValue::new_number(area.width as f64));
                vm.struct_set(area_id, "h", BxValue::new_number(area.height as f64));
                
                // 2. Create RenderingContext
                let ctx = Rc::new(RefCell::new(RenderingContext::new()));
                // Set initial origin to widget area
                ctx.borrow_mut().current_origin = (area.x, area.y);
                
                // 3. Wrap ctx in BxNativeObject
                let ctx_obj_id = vm.native_object_new(ctx.clone());
                
                // 4. Call __render(ctx, area)
                if let Some(obj_id) = obj.as_gc_id() {
                    let _ = vm.native_object_call_method(obj_id, "__render", &[BxValue::new_ptr(ctx_obj_id), BxValue::new_ptr(area_id)]);
                }
                
                // 5. Playback commands
                ctx.borrow().playback(frame);
            }
        }
    }
}

pub trait RenderInArea {
    fn render_in_area(&self, frame: &mut Frame, area: Rect);
}

pub trait RenderInAreaWithRegistry {
    fn render_in_area(&self, vm: &mut dyn BxVM, frame: &mut Frame, area: Rect, widget_registry: &WidgetRegistry);
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

impl RenderInArea for ListWidget {
    fn render_in_area(&self, frame: &mut Frame, area: Rect) {
        use ratatui::style::{Modifier, Style};
        use ratatui::widgets::{List, ListItem, ListState, StatefulWidget};

        let items: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let display_text = match self.style {
                    ListStyle::Plain => item.clone(),
                    ListStyle::Bulleted => format!("  {}", item),
                    ListStyle::Numbered => format!("{}. {}", i + 1, item),
                };
                ListItem::new(display_text)
            })
            .collect();

        let mut list = List::new(items).style(Style::default());
        if let Some(ref symbol) = self.highlight_symbol {
            list = list.highlight_symbol(symbol.as_str());
        } else {
            list = list.highlight_symbol("> ");
        }
        list = list.highlight_style(Style::default().add_modifier(Modifier::BOLD));

        let mut state = ListState::default();
        state.select(Some(self.selected));

        StatefulWidget::render(list, area, frame.buffer_mut(), &mut state);
    }
}

impl RenderInArea for TableWidget {
    fn render_in_area(&self, frame: &mut Frame, area: Rect) {
        use ratatui::layout::Constraint;
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
        table = table.highlight_style(Style::default().add_modifier(Modifier::REVERSED));

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
    fn render_in_area(&self, vm: &mut dyn BxVM, frame: &mut Frame, area: Rect, widget_registry: &WidgetRegistry) {
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

        if let Some(inner_id) = self.inner_widget_id {
            if let Some(inner_widget) = widget_registry.get(inner_id) {
                let inner_area = block.inner(area);
                block.render(area, frame.buffer_mut());
                inner_widget.render_in_area(vm, frame, inner_area, widget_registry);
                return;
            }
        }

        block.render(area, frame.buffer_mut());
    }
}

impl RenderInArea for InputWidget {
    fn render_in_area(&self, frame: &mut Frame, area: Rect) {
        use ratatui::style::Style;
        use ratatui::widgets::{Block, Borders, Paragraph, Widget};

        let display_text = if self.value.is_empty() && !self.placeholder.is_empty() {
            self.placeholder.clone()
        } else {
            self.value.clone()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(self.prompt.clone());

        let paragraph = Paragraph::new(display_text)
            .block(block)
            .style(Style::default());

        paragraph.render(area, frame.buffer_mut());
    }
}

impl RenderInArea for ProgressBarWidget {
    fn render_in_area(&self, frame: &mut Frame, area: Rect) {
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::{Paragraph, Widget};

        let pct = if self.total > 0 {
            (self.completed as f64 / self.total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let start_color = parse_color(self.start_color.as_deref().unwrap_or("cyan"));
        let end_color = parse_color(self.end_color.as_deref().unwrap_or("green"));
        let empty_color = parse_color(self.empty_color.as_deref().unwrap_or("darkgray"));

        let fill_char = self
            .fill_char
            .as_deref()
            .unwrap_or("█")
            .chars()
            .next()
            .unwrap_or('█');
        let empty_char = self
            .empty_char
            .as_deref()
            .unwrap_or("░")
            .chars()
            .next()
            .unwrap_or('░');

        let bar_width = area.width as usize;

        let mut lines: Vec<Line> = Vec::new();

        for row in 0..area.height {
            let mut spans: Vec<Span> = Vec::new();

            for col in 0..bar_width {
                let ratio = col as f64 / bar_width as f64;

                if ratio < pct {
                    let color = lerp_color(start_color, end_color, ratio / pct.max(0.001));
                    spans.push(Span::styled(
                        fill_char.to_string(),
                        Style::default().fg(color),
                    ));
                } else {
                    spans.push(Span::styled(
                        empty_char.to_string(),
                        Style::default().fg(empty_color),
                    ));
                }
            }

            lines.push(Line::from(spans));
        }

        if self.show_label {
            let label = if self.total > 0 {
                format!("{} / {} ({:.0}%)", self.completed, self.total, pct * 100.0)
            } else {
                "0 / 0 (0%)".to_string()
            };

            let center_row = area.height as usize / 2;
            if center_row < lines.len() {
                let label_start = bar_width.saturating_sub(label.len()) / 2;
                let label_style = Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD);

                for (i, ch) in label.chars().enumerate() {
                    let pos = label_start + i;
                    if pos < lines[center_row].spans.len() {
                        lines[center_row].spans[pos] = Span::styled(ch.to_string(), label_style);
                    }
                }
            }
        }

        let paragraph = Paragraph::new(lines);
        paragraph.render(area, frame.buffer_mut());
    }
}

fn lerp_color(a: ratatui::style::Color, b: ratatui::style::Color, t: f64) -> ratatui::style::Color {
    use ratatui::style::Color;
    let t = t.clamp(0.0, 1.0);
    let [ar, ag, ab] = color_to_rgb(a);
    let [br, bg, bb] = color_to_rgb(b);
    let r = (ar as f64 + (br as f64 - ar as f64) * t).round() as u8;
    let g = (ag as f64 + (bg as f64 - ag as f64) * t).round() as u8;
    let b = (ab as f64 + (bb as f64 - ab as f64) * t).round() as u8;
    Color::Rgb(r, g, b)
}

fn color_to_rgb(c: ratatui::style::Color) -> [u8; 3] {
    use ratatui::style::Color;
    match c {
        Color::Black => [0, 0, 0],
        Color::Red => [255, 0, 0],
        Color::Green => [0, 255, 0],
        Color::Yellow => [255, 255, 0],
        Color::Blue => [0, 0, 255],
        Color::Magenta => [255, 0, 255],
        Color::Cyan => [0, 255, 255],
        Color::White => [255, 255, 255],
        Color::Gray => [128, 128, 128],
        Color::DarkGray => [64, 64, 64],
        Color::LightRed => [255, 128, 128],
        Color::LightGreen => [128, 255, 128],
        Color::LightYellow => [255, 255, 128],
        Color::LightBlue => [128, 128, 255],
        Color::LightMagenta => [255, 128, 255],
        Color::LightCyan => [128, 255, 255],
        Color::Rgb(r, g, b) => [r, g, b],
        _ => [128, 128, 128],
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
    static WIDGET_REGISTRY: RefCell<WidgetRegistry> = RefCell::new(WidgetRegistry::new());
}

pub struct WidgetRegistry {
    widgets: HashMap<usize, WidgetKind>,
    next_id: usize,
}

impl WidgetRegistry {
    pub fn new() -> Self {
        Self {
            widgets: HashMap::new(),
            next_id: 1,
        }
    }

    pub fn with_current<F, R>(f: F) -> R
    where
        F: FnOnce(&mut WidgetRegistry) -> R,
    {
        WIDGET_REGISTRY.with(|registry| f(&mut registry.borrow_mut()))
    }

    pub fn insert(&mut self, widget: WidgetKind) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.widgets.insert(id, widget);
        id
    }

    pub fn get(&self, id: usize) -> Option<&WidgetKind> {
        self.widgets.get(&id)
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut WidgetKind> {
        self.widgets.get_mut(&id)
    }

    pub fn remove(&mut self, id: usize) -> Option<WidgetKind> {
        self.widgets.remove(&id)
    }

    pub fn clear(&mut self) {
        self.widgets.clear();
        self.next_id = 1;
    }
}
