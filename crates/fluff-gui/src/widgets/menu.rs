//! Context menu

use crate::colors::{MENU_SEPARATOR, STATIC_BACKGROUND};
use crate::widgets::{MENU_ITEM_BASELINE, MENU_ITEM_HEIGHT, MENU_SEPARATOR_HEIGHT, TEXT_STYLE};
use indexmap::IndexSet;
use kyute::drawing::{Alignment, Anchor, Anchor2D, BorderPosition, Image, Placement, point, vec2};
use kyute::element::WeakElement;
use kyute::element::prelude::*;
use kyute::kurbo::{Insets, Vec2};
use kyute::text::TextLayout;
use kyute::{Element, EventSource, Point, Rect, Size, Window, WindowOptions, text};
use std::collections::BTreeMap;
use std::hash::Hash;

#[derive(Debug, Clone)]
pub struct MenuEntryActivated<ID>(pub ID);

#[derive(Debug, Clone)]
pub struct MenuEntryHighlighted<ID>(pub ID);

enum MenuItemBase {
    Entry {
        icon: Option<Image>,
        label: TextLayout,
        shortcut: Option<TextLayout>,
        shortcut_letter: Option<char>,
    },
    Separator,
}

impl MenuItemBase {
    pub fn measure(&mut self, input: &LayoutInput) -> Size {
        match self {
            MenuItemBase::Entry { label, .. } => {
                label.layout(input.width.available().unwrap_or_default());
                label.size()
            }
            MenuItemBase::Separator => Size::new(input.width.available().unwrap_or_default(), 4.0),
        }
    }
}

pub struct MenuBase {
    items: Vec<MenuItemBase>,
    insets: Insets,
    highlighted: Option<usize>,
}

fn format_menu_label(label: &str) -> (TextLayout, Option<char>) {
    // find '&' and split the string
    if let Some(pos) = label.find('&') {
        let (before, key_after) = label.split_at(pos);
        let mut chars = key_after.chars();
        match chars.next() {
            Some(ch) => {
                let after = chars.as_str();
                let layout = TextLayout::new(&TEXT_STYLE, text!["{before}" {Underline "{ch}"} "{after}"]);
                return (layout, Some(ch));
            }
            None => {}
        }
    }

    (TextLayout::new(&TEXT_STYLE, text!["{label}"]), None)
}

impl MenuBase {
    pub fn new() -> Self {
        MenuBase {
            items: Vec::new(),
            insets: Insets::uniform(4.0),
            highlighted: None,
        }
    }

    fn add_entry(&mut self, icon: Option<Image>, label: &str, shortcut: Option<&str>) -> usize {
        let (label, shortcut_letter) = format_menu_label(label);
        self.items.push(MenuItemBase::Entry {
            icon,
            label,
            shortcut_letter,
            shortcut: shortcut.map(|s| TextLayout::from_str(&TEXT_STYLE, s)),
        });
        self.items.len() - 1
    }

    /// Returns the index of the item at the given point.
    fn item_at_position(&self, this_bounds: Rect, pos: Point) -> Option<usize> {
        let inset_bounds = this_bounds - self.insets;
        if !inset_bounds.contains(pos) {
            return None;
        }
        let mut y = inset_bounds.y0;
        for (i, item) in self.items.iter().enumerate() {
            match item {
                MenuItemBase::Entry { .. } => {
                    if pos.y < y + MENU_ITEM_HEIGHT {
                        return Some(i);
                    }
                    y += MENU_ITEM_HEIGHT;
                }
                MenuItemBase::Separator => {
                    if pos.y < y + MENU_SEPARATOR_HEIGHT {
                        return Some(i);
                    }
                    y += MENU_SEPARATOR_HEIGHT;
                }
            }
        }
        None
    }
}

struct MenuEventResult {
    highlighted_item: Option<usize>,
    activated_item: Option<usize>,
}

const MENU_ICON_PADDING_LEFT: f64 = 4.0;
const MENU_ICON_PADDING_RIGHT: f64 = 4.0;
const MENU_ICON_SIZE: f64 = 16.0;

const MENU_ICON_SPACE: f64 = MENU_ICON_PADDING_LEFT + MENU_ICON_SIZE + MENU_ICON_PADDING_RIGHT;

impl MenuBase {
    fn measure(&mut self, _input: &LayoutInput) -> Size {
        // minimum menu width
        let mut width = 100.0f64;
        let mut height = 0.0f64;
        for item in self.items.iter_mut() {
            match item {
                MenuItemBase::Entry { label, .. } => {
                    label.layout(f64::INFINITY);
                    let size = label.size();
                    width = width.max(MENU_ICON_SPACE + size.width + self.insets.x_value());
                    // TODO measure shortcut
                    // don't care about text height
                    height += MENU_ITEM_HEIGHT;
                }
                MenuItemBase::Separator => {
                    height += MENU_SEPARATOR_HEIGHT;
                }
            }
        }

        eprintln!("MenuBase::measure: width={}, height={}", width, height);

        Size {
            width: width + self.insets.x_value(),
            height: height + self.insets.y_value(),
        }
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {
        for item in self.items.iter_mut() {
            match item {
                MenuItemBase::Entry { label, .. } => {
                    label.layout(size.width - MENU_ICON_SPACE - self.insets.x_value());
                }
                MenuItemBase::Separator => {}
            }
        }
        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: None,
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.rect.contains(point)
    }

    fn event(&mut self, bounds: Rect, event: &mut Event) -> MenuEventResult {
        let mut result = MenuEventResult {
            highlighted_item: None,
            activated_item: None,
        };
        match event {
            Event::PointerMove(event) => {
                // update highlighted item
                let pos = event.local_position();
                let item = self.item_at_position(bounds, pos);
                if self.highlighted != item {
                    self.highlighted = item;
                    result.highlighted_item = item;
                }
            }
            Event::PointerUp(event) => {
                // trigger item
                let pos = event.local_position();
                if let Some(item) = self.item_at_position(bounds, pos) {
                    result.activated_item = Some(item);
                }
            }
            _ => {}
        }
        result
    }

    fn paint(&mut self, ctx: &mut PaintCtx, bounds: Rect) {
        let mut y = self.insets.y0;

        // menu border
        ctx.draw_border(bounds.to_rounded_rect(0.0), 1.0, BorderPosition::Inside, MENU_SEPARATOR);

        for (i, item) in self.items.iter().enumerate() {
            match item {
                MenuItemBase::Entry {
                    icon, label, shortcut, ..
                } => {
                    if self.highlighted == Some(i) {
                        let rect = ctx.snap_rect_to_device_pixel(Rect {
                            x0: bounds.x0 + self.insets.x0,
                            x1: bounds.x1 - self.insets.x1,
                            y0: y,
                            y1: y + MENU_ITEM_HEIGHT,
                        });
                        ctx.fill_rect(rect, MENU_SEPARATOR);
                    }

                    let text_offset_y = y + MENU_ITEM_BASELINE - label.baseline();
                    ctx.draw_text_layout(point(self.insets.x0 + MENU_ICON_SPACE, text_offset_y), label);
                    y += MENU_ITEM_HEIGHT;
                }
                MenuItemBase::Separator => {
                    let mid = y + 0.5 * MENU_SEPARATOR_HEIGHT;
                    ctx.fill_rect(
                        ctx.snap_rect_to_device_pixel(Rect {
                            x0: bounds.x0 + self.insets.x0,
                            x1: bounds.x1 - self.insets.x1,
                            y0: mid - 0.5,
                            y1: mid + 0.5,
                        }),
                        MENU_SEPARATOR,
                    );
                    y += MENU_SEPARATOR_HEIGHT;
                }
            }
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

pub struct Menu<ID> {
    menu: MenuBase,
    items: BTreeMap<usize, ID>,
}

impl<ID: Clone + Eq + Ord + Hash + 'static> Menu<ID> {
    pub fn new() -> ElementBuilder<Self> {
        ElementBuilder::new(Menu {
            menu: MenuBase::new(),
            items: BTreeMap::new(),
        })
    }

    pub fn entry(mut self: ElementBuilder<Self>, label: &str, id: ID) -> ElementBuilder<Self> {
        let index = self.menu.add_entry(None, label, None);
        self.items.insert(index, id);
        self
    }

    pub fn separator(mut self: ElementBuilder<Self>) -> ElementBuilder<Self> {
        self.menu.items.push(MenuItemBase::Separator);
        self
    }
}

impl<ID: Clone + Eq + Ord + Hash + 'static> Element for Menu<ID> {
    fn measure(&mut self, layout_input: &LayoutInput) -> Size {
        self.menu.measure(layout_input)
    }

    fn layout(&mut self, size: Size) -> LayoutOutput {
        self.menu.layout(size)
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        self.menu.hit_test(ctx, point)
    }

    fn paint(self: &mut ElemBox<Self>, ctx: &mut PaintCtx) {
        let bounds = self.ctx.rect();
        self.menu.paint(ctx, bounds)
    }

    fn event(self: &mut ElemBox<Self>, _ctx: &mut WindowCtx, event: &mut Event) {
        let bounds = self.ctx.rect();
        let result = self.menu.event(bounds, event);
        if let Some(item_index) = result.activated_item {
            if let Some(id) = self.items.get(&item_index) {
                self.ctx.emit(MenuEntryActivated(id.clone()));
            }
            self.ctx.mark_needs_paint();
        }
        if let Some(item_index) = result.highlighted_item {
            if let Some(id) = self.items.get(&item_index) {
                self.ctx.emit(MenuEntryHighlighted(id.clone()));
            }
            self.ctx.mark_needs_paint();
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

pub enum MenuItem<'a, ID> {
    Entry { label: &'a str, id: ID },
    Separator,
}

pub struct ContextMenu<ID> {
    window: Window,
    menu: WeakElement<Menu<ID>>,
}

impl<ID: Clone + 'static> ContextMenu<ID> {
    pub async fn entry_activated(&self) -> ID {
        let MenuEntryActivated(id) = self.menu.wait_event().await;
        id
    }

    pub async fn entry_highlighted(&self) -> ID {
        let MenuEntryHighlighted(id) = self.menu.wait_event().await;
        id
    }
}

/// Fit a menu in available space.
///
/// Returns the anchor point on the menu rectangle.
fn calc_menu_anchor(monitor: Size, click: Point, menu: Size) -> Vec2 {
    let x = (click.x + menu.width - monitor.width).max(0.0);
    // y-anchor is either top or bottom, never in the middle
    let y = if click.y + menu.height > monitor.height {
        menu.height
    } else {
        0.0
    };
    Vec2 { x, y }
}

pub fn context_menu<'a, ID: Clone + Eq + Ord + Hash + 'static>(
    parent_window: Window,
    click_position: Point,
    items: impl IntoIterator<Item = MenuItem<'a, ID>>,
) -> ContextMenu<ID> {
    let mut menu = Menu::new();
    for item in items {
        match item {
            MenuItem::Entry { label, id } => {
                menu = menu.entry(label, id.clone());
            }
            MenuItem::Separator => {
                menu = menu.separator();
            }
        }
    }

    let size = menu.measure(&LayoutInput::default());
    let menu_weak = menu.weak();

    // get position in screen coordinates

    let click_screen_position = parent_window.map_to_screen(click_position);
    // TODO wrapper for monitor handles
    // FIXME logical monitor size
    let anchor = if let Some(monitor_size) = parent_window.monitor().map(|m| {
        let size = m.size();
        Size {
            width: size.width as f64,
            height: size.height as f64,
        }
    }) {
        calc_menu_anchor(monitor_size, click_screen_position, size)
    } else {
        Vec2::ZERO
    };
    let menu_position = click_screen_position - anchor;

    // create popup window
    let window = Window::new(
        &WindowOptions {
            title: "",
            size,
            parent: Some(parent_window.raw_window_handle()),
            decorations: false,
            visible: true,
            background: STATIC_BACKGROUND,
            position: Some(menu_position),
            no_focus: false,
        },
        menu,
    );

    ContextMenu {
        window,
        menu: menu_weak,
    }
}
