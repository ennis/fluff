//! Context menu

use crate::colors::{MENU_SEPARATOR, STATIC_BACKGROUND};
use crate::widgets::{MENU_ITEM_BASELINE, MENU_ITEM_HEIGHT, MENU_SEPARATOR_HEIGHT, TEXT_STYLE};
use kyute::application::{run_after, spawn};
use kyute::drawing::{BorderPosition, Image, point};
use kyute::element::WeakElement;
use kyute::element::prelude::*;
use kyute::kurbo::{Insets, Vec2};
use kyute::model::{emit_global, subscribe_global, wait_event_global};
use kyute::text::TextLayout;
use kyute::window::{FocusChanged, Monitor, WindowHandle};
use kyute::{AbortHandle, Element, EventSource, Point, Rect, Size, Window, WindowOptions, text, select};
use std::collections::BTreeMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Range;
use std::rc::Rc;

#[derive(Debug, Clone, Copy)]
pub struct InternalMenuEntryActivated {
    pub index: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct InternalMenuEntryHighlighted {
    pub index: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct InternalMenuCancelled;

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
        Node::Entry {
            item_count: submenu_count,
            ..
        } => index + 1..index + 1 + submenu_count,
        Node::Separator => Range::default(),
    }
}

fn create_menu_popup(mut content: ElementBuilder<MenuBase>, parent_menu: Option<WindowHandle>, menu_position: Point) -> Window {
    // create popup window
    let size = content.measure(&LayoutInput::default());
    let parent_window = content.parent_window.clone();

    // the parent of the menu is the main window,
    // but the menu will be set as a popup of the parent menu
    // not sure if this is necessary
    let window = Window::new(
        &WindowOptions {
            title: "",
            size,
            parent: Some(content.parent_window.raw_window_handle().expect("parent window closed")),
            decorations: false,
            visible: true,
            background: STATIC_BACKGROUND,
            position: Some(menu_position),
            no_focus: true,
        },
        content,
    );

    let popup_parent = parent_menu.unwrap_or(parent_window);
    //popup_parent.set_popup(&window);
    window
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

/// Menu node.
enum Node {
    Entry {
        label: String,
        /// Number of submenu items, 0 for no submenu.
        ///
        /// Submenu item nodes follow immediately after this node.
        item_count: usize,
    },
    Separator,
}

/// Flattened tree of menu items, shared between all submenu windows.
type MenuTree = Rc<Vec<Node>>;

pub struct MenuBase {
    weak_this: WeakElement<Self>,
    parent_window: WindowHandle,
    monitor: Monitor,
    items: Vec<InternalMenuItem>,
    range: Range<usize>,
    tree: MenuTree,
    insets: Insets,
    highlighted: Option<usize>,
    submenu: Option<Window>,
    // Timer for submenu closing on focus loss
    abort_submenu_closing: Option<AbortHandle>,
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
    fn new(parent_window: WindowHandle, monitor: Monitor, tree: MenuTree, range: Range<usize>) -> ElementBuilder<Self> {
        let mut items = Vec::new();
        let mut i = range.start;
        while i < range.end {
            match &tree[i] {
                Node::Entry {
                    label,
                    item_count: submenu_count,
                } => {
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
            abort_submenu_closing: None,
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
        create_menu_popup(self, None, position)
    }

    /// Opens a submenu.
    fn open_submenu(&mut self, cx: &ElementCtx, display_rect: Rect, range: Range<usize>) {
        let mut submenu = MenuBase::new(
            self.parent_window.clone(),
            self.monitor.clone(),
            self.tree.clone(),
            range,
        ).set_focus();
        let size = submenu.measure(&LayoutInput::default());
        let position = calc_menu_position(self.monitor.logical_size(), display_rect, size, false);
        let popup = create_menu_popup(submenu, Some(cx.get_parent_window()), position);

        // Close menu when focus is lost
        let weak_this = self.weak_this.clone();
        popup.subscribe(move |&FocusChanged(focused)| {
            if !focused {
                eprintln!("Submenu focus lost");
                if let Some(this) = weak_this.upgrade() {
                    this.borrow_mut().submenu = None;
                }
                false
            } else {
                true
            }
        });

        // Cancel submenu close timer
        if let Some(submenu_closing) = self.abort_submenu_closing.take() {
            submenu_closing.abort();
        }

        // This will close any existing submenu
        self.submenu = Some(popup);
    }

    /// Closes the currently opened submenu after a delay.
    fn close_submenu_delayed(&mut self) {
        if self.submenu.is_none() {
            return;
        }
        // there's already a close pending
        if self.abort_submenu_closing.is_some() {
            return;
        }

        let weak_this = self.weak_this.clone();
        let abort = run_after(SUBMENU_CLOSE_DELAY, move || {
            if let Some(this) = weak_this.upgrade() {
                let mut this = this.borrow_mut();
                this.submenu = None;
                this.abort_submenu_closing = None;
            }
        });

        self.abort_submenu_closing = Some(abort);
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
        Rect::from_origin_size(self.parent_window.map_to_screen(rect.origin()), rect.size())
    }
}

const MENU_ICON_PADDING_LEFT: f64 = 4.0;
const MENU_ICON_PADDING_RIGHT: f64 = 4.0;
const MENU_ICON_SIZE: f64 = 16.0;
const MENU_ICON_SPACE: f64 = MENU_ICON_PADDING_LEFT + MENU_ICON_SIZE + MENU_ICON_PADDING_RIGHT;
const SUBMENU_CLOSE_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

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

    fn paint(&mut self, cx: &ElementCtx, ctx: &mut PaintCtx) {
        let bounds = cx.rect();
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

    fn event(&mut self, cx: &ElementCtx, event: &mut Event) {
        let bounds = cx.rect();
        match event {
            Event::PointerMove(event) => {
                // update highlighted item
                let pos = event.local_position();
                if let Some((index, entry_bounds)) = self.entry_at_position(bounds, pos) {
                    if self.highlighted != Some(index) {
                        self.highlighted = Some(index);
                        emit_global(InternalMenuEntryHighlighted { index });

                        // If the newly highlighted item is a submenu, open it
                        let range = submenu_range(&self.tree, index);
                        if !range.is_empty() {
                            let rect =
                                Rect::from_origin_size(cx.map_to_monitor(entry_bounds.origin()), entry_bounds.size());
                            self.open_submenu(cx, rect, range);
                            cx.mark_needs_paint();
                        } else {
                            // Otherwise, wait & close the current submenu
                            self.close_submenu_delayed();
                        }
                    }
                } else {
                    if self.highlighted.is_some() {
                        self.highlighted = None;
                        cx.mark_needs_paint();
                    }
                }
            }
            Event::PointerUp(event) => {
                // trigger item
                let pos = event.local_position();
                if let Some((item, _bounds)) = self.entry_at_position(bounds, pos) {
                    emit_global(InternalMenuEntryActivated { index: item });
                    cx.mark_needs_paint();
                }
            }
            Event::KeyDown(event) => {
                match event.key {
                    kyute::event::Key::Escape => {
                        emit_global(InternalMenuCancelled);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Clone)]
pub enum MenuItem<'a, ID> {
    Entry(&'a str, ID),
    Submenu(&'a str, &'a [MenuItem<'a, ID>]),
    Separator,
}

fn flatten_menu_rec<ID>(
    items: &[MenuItem<ID>],
    base_index: &mut usize,
    index_to_id: &mut BTreeMap<usize, ID>,
) -> Vec<Node>
where
    ID: Clone,
{
    let mut flat = Vec::new();
    for item in items.iter() {
        match item {
            MenuItem::Entry(label, id) => {
                index_to_id.insert(*base_index, id.clone());
                flat.push(Node::Entry {
                    label: (*label).to_owned(),
                    item_count: 0,
                });
                *base_index += 1;
            }
            MenuItem::Submenu(label, items) => {
                *base_index += 1;
                let items_flat = flatten_menu_rec(items, base_index, index_to_id);
                flat.push(Node::Entry {
                    label: (*label).to_owned(),
                    item_count: items_flat.len(),
                });
                flat.extend(items_flat);
            }
            MenuItem::Separator => {
                flat.push(Node::Separator);
                *base_index += 1;
            }
        }
    }
    flat
}

/// Flattens a subtree of `MenuItem`s into a flat list of `Node`s.
///
/// `index_to_id` is a map from node index to corresponding entry ID.
fn flatten_menu<ID>(items: &[MenuItem<ID>]) -> (Vec<Node>, BTreeMap<usize, ID>)
where
    ID: Clone,
{
    let mut base_index = 0;
    let mut index_to_id = BTreeMap::new();
    let flat = flatten_menu_rec(items, &mut base_index, &mut index_to_id);
    (flat, index_to_id)
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Represents an open context menu.
///
/// Drop to close the context menu.
pub struct ContextMenu<ID> {
    parent_window: WindowHandle,
    popup: Window,
    index_to_id: BTreeMap<usize, ID>,
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

    pub async fn cancelled(&self) {
        select! {
            _ = wait_event_global::<InternalMenuCancelled>() => {}
            _ = self.popup.close_requested() => {}
            _ = self.parent_window.popup_cancelled() => {}
        }
    }

    /*pub async fn close_requested(&self) {
        self.popup.close_requested().await;
    }*/
}

fn open_context_menu_popup(parent_window: WindowHandle, click_position: Point, tree: MenuTree) -> Window {
    let range = 0..tree.len();
    let monitor = parent_window.monitor().unwrap();
    let popup = MenuBase::new(parent_window, monitor, tree, range)
        .set_focus()
        .open(click_position);
    popup
}

pub fn context_menu<ID: Clone + 'static>(
    parent_window: WindowHandle,
    click_position: Point,
    items: &[MenuItem<ID>],
) -> ContextMenu<ID> {
    let (tree, index_to_id) = flatten_menu(items);
    let popup = open_context_menu_popup(parent_window.clone(), click_position, Rc::new(tree));
    ContextMenu { parent_window, index_to_id, popup }
}

/// Extension trait on `ElementCtx` to open a context menu.
pub trait ContextMenuExt {
    /// Opens a context menu at the specified position.
    fn open_context_menu<ID: Clone + 'static>(&self, click_position: Point, items: &[MenuItem<ID>],
                                              on_entry_activated: impl FnOnce(ID) + 'static,);
}

impl ContextMenuExt for ElementCtx {
    fn open_context_menu<ID: Clone + 'static>(
        &self,
        click_position: Point,
        items: &[MenuItem<ID>],
        on_entry_activated: impl FnOnce(ID) + 'static,
    )
    {
        let parent_window = self.get_parent_window();
        let (tree, index_to_id) = flatten_menu(items);
        // tree is shared between all submenu windows
        let tree = Rc::new(tree);
        spawn(async move {
            let menu = ContextMenu {
                parent_window: parent_window.clone(),
                index_to_id,
                popup: open_context_menu_popup(parent_window, click_position, tree),
            };
            // FIXME: there's no way to cancel it?
            select! {
                id = menu.entry_activated() => {
                    on_entry_activated(id);
                }
                _ = menu.cancelled() => {
                    eprintln!("Context menu cancelled");
                }
            }
        });

    }
}
