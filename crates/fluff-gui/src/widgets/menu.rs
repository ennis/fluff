//! Context menu

use crate::colors::{MENU_SEPARATOR, STATIC_BACKGROUND};
use crate::widgets::{MENU_ITEM_BASELINE, MENU_ITEM_HEIGHT, MENU_SEPARATOR_HEIGHT, TEXT_STYLE};
use kyute::application::spawn;
use kyute::drawing::{BorderPosition, Image, point};
use kyute::element::prelude::*;
use kyute::kurbo::{Insets, Vec2};
use kyute::model::{emit_global, subscribe_global, wait_event_global};
use kyute::text::TextLayout;
use kyute::window::{FocusChanged, Monitor};
use kyute::{Element, Point, Rect, Size, Window, WindowOptions, text, EventSource};
use std::collections::BTreeMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Range;
use std::rc::Rc;
use kyute::element::WeakElement;

#[derive(Debug, Clone, Copy)]
pub struct InternalMenuEntryActivated {
    pub index: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct InternalMenuEntryHighlighted {
    pub index: usize,
}

enum InternalMenuItem {
    Entry {
        index: usize,
        icon: Option<Image>,
        label: TextLayout,
        shortcut: Option<TextLayout>,
        shortcut_letter: Option<char>,
        submenu: bool,
    },
    Separator,
}

impl InternalMenuItem {
    pub fn measure(&mut self, input: &LayoutInput) -> Size {
        match self {
            InternalMenuItem::Entry { label, .. } => {
                label.layout(input.width.available().unwrap_or_default());
                label.size()
            }
            InternalMenuItem::Separator => Size::new(input.width.available().unwrap_or_default(), 4.0),
        }
    }
}

fn submenu_range(nodes: &[Node], index: usize) -> Range<usize> {
    match nodes[index] {
        Node::Entry { submenu_count, .. } => index + 1..index + 1 + submenu_count,
        Node::Separator => Range::default(),
    }
}

fn flatten_menu_items<ID: Clone>(
    items: &[MenuItem<ID>],
    mut base_index: &mut usize,
    index_to_id: &mut BTreeMap<usize, ID>,
) -> Vec<Node> {
    let mut flat = Vec::new();
    for item in items.iter() {
        match item {
            MenuItem::Entry { label, id, submenu } => {
                index_to_id.insert(*base_index, id.clone());
                *base_index += 1;
                let submenu_flat = flatten_menu_items(submenu, base_index, index_to_id);
                flat.push(Node::Entry {
                    label: label.clone(),
                    submenu_count: submenu_flat.len(),
                });
                flat.extend(submenu_flat);
            }
            MenuItem::Separator => {
                flat.push(Node::Separator);
                *base_index += 1;
            }
        }
    }
    flat
}

fn create_menu_popup(mut content: ElementBuilder<MenuBase>, menu_position: Point) -> Window {
    // create popup window
    let size = content.measure(&LayoutInput::default());
    Window::new(
        &WindowOptions {
            title: "",
            size,
            parent: Some(content.parent_window.raw_window_handle()),
            decorations: false,
            visible: true,
            background: STATIC_BACKGROUND,
            position: Some(menu_position),
            no_focus: false,
        },
        content,
    )
}

/// Fit a menu in available space.
///
/// Returns the anchor point of the top-left corner of the menu.
fn calc_menu_position(monitor: Size, rect: Rect, menu: Size, allow_x_overlap: bool) -> Point {
    let mut x = rect.x1;
    if x + menu.width > monitor.width {
        // overflows on the right
        if allow_x_overlap {
            // shift menu to the left
            x = rect.x1 - (x + menu.width - monitor.width);
        } else {
            // place menu to the left
            x = rect.x1 - menu.width;
        }
        x = x.max(0.0);
    }
    let mut y = rect.y0;
    if y + menu.height > monitor.height {
        // overflows on the bottom
        y = rect.y1 - menu.height;
        y = y.max(0.0);
    }
    Point { x, y }
}

enum Node {
    Entry { label: String, submenu_count: usize },
    Separator,
}

pub struct MenuBase {
    weak_this: WeakElement<Self>,
    parent_window: Window,
    monitor: Monitor,
    items: Vec<InternalMenuItem>,
    range: Range<usize>,
    tree: Rc<Vec<Node>>,
    insets: Insets,
    highlighted: Option<usize>,
    submenu: Option<Window>,
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
    fn new(parent_window: Window, monitor: Monitor, tree: Rc<Vec<Node>>, range: Range<usize>) -> ElementBuilder<Self> {
        let mut items = Vec::new();
        let mut i = range.start;
        while i < range.end {
            match &tree[i] {
                Node::Entry { label, submenu_count } => {
                    let (label, shortcut_letter) = format_menu_label(&label);
                    items.push(InternalMenuItem::Entry {
                        icon: None,
                        index: i,
                        label,
                        shortcut: None,
                        shortcut_letter,
                        submenu: *submenu_count > 0,
                    });
                    i += 1 + *submenu_count;
                }
                Node::Separator => {
                    items.push(InternalMenuItem::Separator);
                    i += 1;
                }
            }
        }

        ElementBuilder::new_cyclic(|weak_this| MenuBase {
            weak_this,
            parent_window,
            monitor,
            items,
            range,
            tree,
            insets: Insets::uniform(4.0),
            highlighted: None,
            submenu: None,
        })
    }

    fn open(mut self: ElementBuilder<Self>, at: Point) -> Window {
        let size = self.measure(&LayoutInput::default());
        let at_display = self.parent_window.map_to_screen(at);
        let position = calc_menu_position(
            self.monitor.logical_size(),
            Rect::from_origin_size(at_display, Size::ZERO),
            size,
            false,
        );
        create_menu_popup(self, position)
    }

    fn open_submenu(&mut self, display_rect: Rect, index: usize) {
        let range = submenu_range(&self.tree, index);
        if range.is_empty() {
            return;
        }

        let mut submenu = MenuBase::new(
            self.parent_window.clone(),
            self.monitor.clone(),
            self.tree.clone(),
            range,
        );
        let size = submenu.measure(&LayoutInput::default());
        let position = calc_menu_position(self.monitor.logical_size(), display_rect, size, false);
        let popup = create_menu_popup(submenu, position);

        // Close menu when focus is lost
        let weak_this = self.weak_this.clone();
        popup.subscribe(move |&FocusChanged(focused)| {
            if !focused {
                if let Some(this) = weak_this.upgrade() {
                    this.borrow_mut().submenu = None;
                }
                false
            } else {
                true
            }
        });

        self.submenu = Some(popup);
    }

    /// Returns the index and the bounds of the entry at the given point.
    ///
    /// Returns None if the point falls on a separator or outside the menu.
    fn entry_at_position(&self, this_bounds: Rect, pos: Point) -> Option<(usize, Rect)> {
        let inset_bounds = this_bounds - self.insets;
        if !inset_bounds.contains(pos) {
            return None;
        }
        let mut y = inset_bounds.y0;
        for item in self.items.iter() {
            match item {
                InternalMenuItem::Entry { index, .. } => {
                    if pos.y < y + MENU_ITEM_HEIGHT {
                        let rect = Rect {
                            x0: inset_bounds.x0,
                            x1: inset_bounds.x1,
                            y0: y,
                            y1: y + MENU_ITEM_HEIGHT,
                        };
                        return Some((*index, rect));
                    }
                    y += MENU_ITEM_HEIGHT;
                }
                InternalMenuItem::Separator => {
                    if pos.y < y + MENU_SEPARATOR_HEIGHT {
                        return None;
                    }
                    y += MENU_SEPARATOR_HEIGHT;
                }
            }
        }
        None
    }

    fn rect_to_display(&self, rect: Rect) -> Rect {
        Rect::from_origin_size(
            self.parent_window.map_to_screen(rect.origin()),
            rect.size(),
        )
    }
}


const MENU_ICON_PADDING_LEFT: f64 = 4.0;
const MENU_ICON_PADDING_RIGHT: f64 = 4.0;
const MENU_ICON_SIZE: f64 = 16.0;
const MENU_ICON_SPACE: f64 = MENU_ICON_PADDING_LEFT + MENU_ICON_SIZE + MENU_ICON_PADDING_RIGHT;

impl Element for MenuBase {
    fn measure(&mut self, _input: &LayoutInput) -> Size {
        // minimum menu width
        let mut width = 100.0f64;
        let mut height = 0.0f64;
        for item in self.items.iter_mut() {
            match item {
                InternalMenuItem::Entry { label, .. } => {
                    label.layout(f64::INFINITY);
                    let size = label.size();
                    width = width.max(MENU_ICON_SPACE + size.width + self.insets.x_value());
                    // TODO measure shortcut
                    // don't care about text height
                    height += MENU_ITEM_HEIGHT;
                }
                InternalMenuItem::Separator => {
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
                InternalMenuItem::Entry { label, .. } => {
                    label.layout(size.width - MENU_ICON_SPACE - self.insets.x_value());
                }
                InternalMenuItem::Separator => {}
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

    fn paint(self: &mut ElemBox<Self>, ctx: &mut PaintCtx) {
        let bounds = self.ctx.rect();
        let mut y = self.insets.y0;

        // menu border
        ctx.draw_border(bounds.to_rounded_rect(0.0), 1.0, BorderPosition::Inside, MENU_SEPARATOR);

        for item in self.items.iter() {
            match &item {
                InternalMenuItem::Entry {
                    icon,
                    label,
                    shortcut,
                    index,
                    ..
                } => {
                    if self.highlighted == Some(*index) {
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
                InternalMenuItem::Separator => {
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


    fn event(self: &mut ElemBox<Self>, event: &mut Event) {
        let bounds = self.ctx.rect();
        match event {
            Event::PointerMove(event) => {
                // update highlighted item
                let pos = event.local_position();
                if let Some((index, entry_bounds)) = self.entry_at_position(bounds, pos) {
                    if self.highlighted != Some(index) {
                        self.highlighted = Some(index);
                        emit_global(InternalMenuEntryHighlighted {
                            index,
                        });

                        let rect = Rect::from_origin_size(self.ctx.map_to_monitor(entry_bounds.origin()), entry_bounds.size());
                        self.open_submenu(rect, index);
                        self.ctx.mark_needs_paint();
                    }
                } else {
                    if self.highlighted.is_some() {
                        self.highlighted = None;
                        self.ctx.mark_needs_paint();
                    }
                }

            }
            Event::PointerUp(event) => {
                // trigger item
                let pos = event.local_position();
                if let Some((item, bounds)) = self.entry_at_position(bounds, pos) {
                    emit_global(InternalMenuEntryActivated {
                        index: item,
                    });
                    self.ctx.mark_needs_paint();
                }
            }
            _ => {}
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub enum MenuItem<ID> {
    Entry {
        label: String,
        id: ID,
        submenu: Vec<MenuItem<ID>>,
    },
    Separator,
}

pub struct ContextMenu<ID> {
    popup: Window,
    index_to_id: BTreeMap<usize, ID>,
    _phantom: PhantomData<ID>,
}

impl<ID: Clone + 'static> ContextMenu<ID> {
    pub async fn entry_activated(&self) -> ID {
        // There should be only one context menu with a specific ID type active at any given time,
        // so it should be OK to use a global event here.
        loop {
            let InternalMenuEntryActivated { index, .. } = wait_event_global().await;
            if let Some(id) = self.index_to_id.get(&index) {
                return id.clone();
            }
        }
    }

    pub async fn entry_highlighted(&self) -> ID {
        loop {
            let InternalMenuEntryHighlighted { index, .. } = wait_event_global().await;
            if let Some(id) = self.index_to_id.get(&index) {
                return id.clone();
            }
        }
    }
}

pub fn context_menu<'a, ID: Clone + 'static>(
    parent_window: Window,
    click_position: Point,
    items: impl IntoIterator<Item = MenuItem<ID>>,
) -> ContextMenu<ID> {
    let mut index_to_id = BTreeMap::new();
    let tree = Rc::new(flatten_menu_items(
        &items.into_iter().collect::<Vec<_>>(),
        &mut 0,
        &mut index_to_id,
    ));
    let range = 0..tree.len();
    let monitor = parent_window.monitor().unwrap();
    let popup = MenuBase::new(parent_window, monitor, tree, range).open(click_position);

    ContextMenu {
        index_to_id,
        popup,
        _phantom: PhantomData,
    }
}
