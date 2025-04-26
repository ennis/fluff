//! Context menu

use crate::colors::{MENU_SEPARATOR, STATIC_BACKGROUND, STATIC_TEXT};
use crate::widgets::{MENU_ITEM_BASELINE, MENU_ITEM_HEIGHT, MENU_SEPARATOR_HEIGHT, TEXT_STYLE};
use kyute::application::{run_after, spawn, CallbackToken};
use kyute::drawing::{BorderPosition, Image, point, vec2};
use kyute::element::prelude::*;
use kyute::element::{TreeCtx, WeakElement};
use kyute::kurbo::PathEl::{LineTo, MoveTo};
use kyute::kurbo::{Insets, Vec2};
use kyute::event::{emit_global, subscribe_global, wait_event_global};
use kyute::text::TextLayout;
use kyute::window::{FocusChanged, PopupPlacement, WindowHandle, place_popup};
use kyute::{AbortHandle, Element, EventSource, Point, Rect, Size, Window, WindowOptions, select, text};
use std::collections::BTreeMap;
use std::ops::Range;
use std::rc::Rc;
use tracing::warn;
use kyute::platform::PlatformWindowHandle;

/// Event emitted when a menu entry is activated.
#[derive(Debug, Clone, Copy)]
pub struct MenuEntryActivated<ID>(pub ID);

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

#[allow(dead_code)]
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
    /*fn measure(&mut self, input: &LayoutInput) -> Size {
        match self {
            InternalMenuItem::Entry { label, .. } => {
                label.layout(input.width.available().unwrap_or_default());
                label.size()
            }
            InternalMenuItem::Separator => Size::new(input.width.available().unwrap_or_default(), 4.0),
        }
    }*/
}

fn submenu_range(nodes: &[MenuItemNode], index: usize) -> Range<usize> {
    match nodes[index] {
        MenuItemNode::Entry {
            item_count: submenu_count,
            ..
        } => index + 1..index + 1 + submenu_count,
        MenuItemNode::Separator => Range::default(),
    }
}

fn open_anchored_popup<T: Element>(
    owner: PlatformWindowHandle,
    content: ElementBuilder<T>,
    anchor_rect: Rect,
    popup_placement: PopupPlacement,
) -> Window {
    // create popup window
    let size = content.measure(&LayoutInput::default());
    let position = place_popup(Some(owner.monitor()), size, anchor_rect, popup_placement);

    let window = Window::new(
        &WindowOptions {
            size,
            owner: Some(owner.clone()),
            decorations: false,
            visible: true,
            background: STATIC_BACKGROUND,
            position: Some(position),
            no_focus: true,
            ..Default::default()
        },
        content,
    );

    //let popup_parent = parent_menu.unwrap_or(parent_window);
    // not sure if this is necessary
    //popup_parent.set_popup(&window);
    
    window
}

/// Menu item node within a `MenuItemNodeRange`.
pub enum MenuItemNode {
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
#[derive(Clone)]
pub struct MenuItemNodeRange {
    nodes: Rc<Vec<MenuItemNode>>,
    range: Range<usize>,
}

impl MenuItemNodeRange {
    /// Iterates over the items in this subtree (not recursively).
    pub fn iter(&self) -> impl Iterator<Item = (usize, &MenuItemNode)> {
        let mut i = self.range.start;
        std::iter::from_fn(move || {
            if i < self.range.end {
                let node = &self.nodes[i];
                let index = i;
                match node {
                    MenuItemNode::Entry { item_count, .. } => {
                        i += 1 + *item_count;
                    }
                    MenuItemNode::Separator => {
                        i += 1;
                    }
                }
                Some((index, node))
            } else {
                None
            }
        })
    }

    /// Returns the submenu at the given index, or None if the item has no submenu.
    pub fn child_item_range(&self, index: usize) -> Option<MenuItemNodeRange> {
        let range = submenu_range(&self.nodes, index);
        if range.is_empty() {
            None
        } else {
            Some(MenuItemNodeRange {
                nodes: self.nodes.clone(),
                range,
            })
        }
    }

    /// Returns whether the item at index has submenu items.
    pub fn has_child_items(&self, index: usize) -> bool {
        !submenu_range(&self.nodes, index).is_empty()
    }
}

pub struct MenuBase {
    weak_this: WeakElement<Self>,
    owner: PlatformWindowHandle,
    items: Vec<InternalMenuItem>,
    tree: MenuItemNodeRange,
    insets: Insets,
    highlighted: Option<usize>,
    submenu: Option<Window>,
    // Timer for submenu closing on focus loss
    abort_submenu_closing: Option<CallbackToken>,
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
    fn new(owner: PlatformWindowHandle, tree: MenuItemNodeRange) -> ElementBuilder<Self> {
        let mut items = Vec::new();

        for (index, item) in tree.iter() {
            match item {
                MenuItemNode::Entry { label, item_count } => {
                    let (label, shortcut_letter) = format_menu_label(&label);
                    items.push(InternalMenuItem::Entry {
                        icon: None,
                        index,
                        label,
                        shortcut: None,
                        shortcut_letter,
                        submenu: *item_count > 0,
                    });
                }
                MenuItemNode::Separator => {
                    items.push(InternalMenuItem::Separator);
                }
            }
        }

        ElementBuilder::new_cyclic(|weak_this| MenuBase {
            weak_this,
            owner,
            items,
            tree,
            insets: Insets::uniform(4.0),
            highlighted: None,
            submenu: None,
            abort_submenu_closing: None,
        })
    }

    /*
    /// Opens a menu at the specified position.
    fn open(self: ElementBuilder<Self>, at: Point, popup_placement: PopupPlacement) -> Window {
        self.open_around(Rect::from_origin_size(at, Size::ZERO), popup_placement)
    }*/

    /// Opens a menu around the specified rectangle.
    ///
    ///  # Arguments
    /// * `rect` - The bounding rectangle of the parent menu item, in the coordinate space of
    ///            the monitor or the parent window (`self.parent_window`).
    fn open_around(self: ElementBuilder<Self>, rect: Rect, popup_placement: PopupPlacement) -> Window {
        // round size to device pixels
        //let scale_factor = self.parent_window.scale_factor();
        //let size = Size::new(
        //    round_to_device_pixel(size.width, scale_factor),
        //    round_to_device_pixel(size.height, scale_factor),
        //);
        open_anchored_popup(self.owner.clone(), self, rect, popup_placement)
    }

    /// Opens a submenu.
    ///
    /// # Arguments
    /// * `cx` - The element context.
    /// * `around` - The bounding rectangle of the parent menu item, in the coordinate space of the
    ///              parent window (`self.parent_window`).
    fn open_submenu(&mut self, _cx: &ElementCtx, around: Rect, range: MenuItemNodeRange) {
        let submenu = MenuBase::new(self.owner.clone(), range).set_focus();
        let popup = submenu.open_around(around, PopupPlacement::RightThenLeft);

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
            submenu_closing.cancel();
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

    /*fn rect_to_display(&self, rect: Rect) -> Rect {
        Rect::from_origin_size(self.parent_window.map_to_screen(rect.origin()), rect.size())
    }*/
}

const MENU_ICON_PADDING_LEFT: f64 = 4.0;
const MENU_ICON_PADDING_RIGHT: f64 = 4.0;
const MENU_ICON_SIZE: f64 = 16.0;
const MENU_ICON_SPACE: f64 = MENU_ICON_PADDING_LEFT + MENU_ICON_SIZE + MENU_ICON_PADDING_RIGHT;
const MENU_SUBMENU_ARROW_SPACE: f64 = 16.0;
const SUBMENU_CLOSE_DELAY: std::time::Duration = std::time::Duration::from_millis(500);

impl Element for MenuBase {
    fn measure(&mut self, _cx: &TreeCtx, _input: &LayoutInput) -> Size {
        // minimum menu width
        let mut width = 100.0f64;
        let mut height = 0.0f64;
        for item in self.items.iter_mut() {
            match item {
                InternalMenuItem::Entry { label, .. } => {
                    label.layout(f64::INFINITY);
                    let size = label.size();
                    width = width.max(MENU_ICON_SPACE + size.width + MENU_SUBMENU_ARROW_SPACE + self.insets.x_value());
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

    fn layout(&mut self, _cx: &TreeCtx, size: Size) -> LayoutOutput {
        for item in self.items.iter_mut() {
            match item {
                InternalMenuItem::Entry { label, .. } => {
                    label.layout(size.width - MENU_ICON_SPACE - MENU_SUBMENU_ARROW_SPACE - self.insets.x_value());
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
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, cx: &TreeCtx, ctx: &mut PaintCtx) {
        let bounds = cx.bounds();
        let mut y = self.insets.y0;

        // menu border
        ctx.draw_border(bounds.to_rounded_rect(0.0), 1.0, BorderPosition::Inside, MENU_SEPARATOR);

        for item in self.items.iter() {
            match &item {
                InternalMenuItem::Entry {
                    icon: _,
                    label,
                    shortcut: _,
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
                    ctx.draw_text_layout(
                        point(bounds.x0 + self.insets.x0 + MENU_ICON_SPACE, bounds.y0 + text_offset_y),
                        label,
                    );

                    if self.tree.has_child_items(*index) {
                        // draw submenu arrow
                        let arrow_tip = ctx.round_point_to_device_pixel_center(point(
                            bounds.x1 - self.insets.x1 - 3.0,
                            y + 0.5 * MENU_ITEM_HEIGHT,
                        ));
                        ctx.stroke_path(
                            [
                                MoveTo(arrow_tip + vec2(-4., -4.)),
                                LineTo(arrow_tip),
                                LineTo(arrow_tip + vec2(-4., 4.)),
                            ],
                            1.,
                            STATIC_TEXT,
                        );
                    }

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

    fn event(&mut self, cx: &TreeCtx, event: &mut Event) {
        let bounds = cx.bounds();
        match event {
            Event::PointerMove(event) => {
                // update highlighted item
                let pos = event.position;
                if let Some((index, entry_bounds)) = self.entry_at_position(bounds, pos) {
                    if self.highlighted != Some(index) {
                        self.highlighted = Some(index);
                        emit_global(InternalMenuEntryHighlighted { index });

                        // If the newly highlighted item is a submenu, open it
                        if let Some(submenu_items) = self.tree.child_item_range(index) {
                            // TODO: open submenu after a delay
                            let rect = cx.map_rect_to_monitor(entry_bounds); // Rect::from_origin_size(cx.map_to_monitor(entry_bounds.origin()), entry_bounds.size());
                            self.open_submenu(cx, rect, submenu_items);
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
                let pos = event.position;
                if let Some((item, _bounds)) = self.entry_at_position(bounds, pos) {
                    emit_global(InternalMenuEntryActivated { index: item });
                    cx.mark_needs_paint();
                }
            }
            Event::KeyDown(event) => match event.key {
                kyute::input_event::Key::Escape => {
                    emit_global(InternalMenuCancelled);
                }
                _ => {}
            },
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
) -> Vec<MenuItemNode>
where
    ID: Clone,
{
    let mut flat = Vec::new();
    for item in items.iter() {
        match item {
            MenuItem::Entry(label, id) => {
                index_to_id.insert(*base_index, id.clone());
                flat.push(MenuItemNode::Entry {
                    label: (*label).to_owned(),
                    item_count: 0,
                });
                *base_index += 1;
            }
            MenuItem::Submenu(label, items) => {
                *base_index += 1;
                let items_flat = flatten_menu_rec(items, base_index, index_to_id);
                flat.push(MenuItemNode::Entry {
                    label: (*label).to_owned(),
                    item_count: items_flat.len(),
                });
                flat.extend(items_flat);
            }
            MenuItem::Separator => {
                flat.push(MenuItemNode::Separator);
                *base_index += 1;
            }
        }
    }
    flat
}

/// Flattens a subtree of `MenuItem`s into a flat list of `Node`s.
///
/// `index_to_id` is a map from node index to corresponding entry ID.
fn flatten_menu<ID>(items: &[MenuItem<ID>]) -> (MenuItemNodeRange, BTreeMap<usize, ID>)
where
    ID: Clone,
{
    let mut base_index = 0;
    let mut index_to_id = BTreeMap::new();
    let flat = flatten_menu_rec(items, &mut base_index, &mut index_to_id);
    let count = flat.len();
    let range = MenuItemNodeRange {
        nodes: Rc::new(flat),
        range: 0..count,
    };
    (range, index_to_id)
}

////////////////////////////////////////////////////////////////////////////////////////////////////

/// Represents an open context menu.
///
/// Drop to close the context menu.
pub struct ContextMenu<ID> {
    owner: PlatformWindowHandle,
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
            //_ = self.owner.popup_cancelled() => {}
        }
    }

    /*pub async fn close_requested(&self) {
        self.popup.close_requested().await;
    }*/
}

// TODO remove this
fn open_context_menu_popup(owner: PlatformWindowHandle, around_rect_monitor: Rect, items: MenuItemNodeRange) -> Window {
    let popup = MenuBase::new(owner, items)
        .set_focus()
        .open_around(around_rect_monitor, PopupPlacement::RightOrOverlap);
    popup
}

/// Extension trait on `ElementCtx` to open a context menu.
pub trait ContextMenuExt {
    /// Opens a context menu at the specified position.
    ///
    /// # Arguments
    /// * `click_position` - The position of the click in the coordinate space of the parent window.
    fn open_context_menu<ID: Clone + 'static>(
        &self,
        click_position: Point,
        items: &[MenuItem<ID>],
        on_entry_activated: impl FnOnce(ID) + 'static,
    );

    fn open_context_menu_around<ID: Clone + 'static>(
        &self,
        rect: Rect,
        items: &[MenuItem<ID>],
        on_entry_activated: impl FnOnce(ID) + 'static,
    );
}

impl ContextMenuExt for TreeCtx<'_> {
    fn open_context_menu<ID: Clone + 'static>(
        &self,
        click_position: Point,
        items: &[MenuItem<ID>],
        on_entry_activated: impl FnOnce(ID) + 'static,
    ) {
        self.open_context_menu_around(
            Rect::from_origin_size(click_position, Size::ZERO),
            items,
            on_entry_activated,
        );
    }

    fn open_context_menu_around<ID: Clone + 'static>(
        &self,
        rect: Rect,
        items: &[MenuItem<ID>],
        on_entry_activated: impl FnOnce(ID) + 'static,
    ) {
        let owner = self.get_window().platform_window().unwrap();
        let (nodes, index_to_id) = flatten_menu(items);
        let rect_monitor = self.map_rect_to_monitor(rect);
        spawn(async move {
            let menu = ContextMenu {
                owner: owner.clone(),
                index_to_id,
                popup: open_context_menu_popup(owner, rect_monitor, nodes),
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

////////////////////////////////////////////////////////////////////////////////////////////////////

// Menu bar

/// Menu bar widget.
///
/// Emits a global event of type `InternalMenuEntryActivated` when an entry is activated.
#[allow(dead_code)]
pub struct MenuBar<ID> {
    weak_this: WeakElement<Self>,
    entries: Vec<MenuBarEntry>,
    nodes: MenuItemNodeRange,
    index_to_id: BTreeMap<usize, ID>,
    highlighted: Option<usize>,
    menu: Option<Window>,
}

#[allow(dead_code)]
struct MenuBarEntry {
    // bounds in local coord space
    bounds: Rect,
    title: TextLayout,
    shortcut_letter: Option<char>,
    // relative to menu bar local space (not this entry's bounds)
    title_offset: Vec2,
    index: usize,
}

impl<ID: Clone + 'static> MenuBar<ID> {
    pub fn new(items: &[MenuItem<ID>]) -> ElementBuilder<MenuBar<ID>> {
        let (nodes, index_to_id) = flatten_menu(items);
        let mut entries = Vec::new();
        for (index, item_node) in nodes.iter() {
            match item_node {
                MenuItemNode::Entry { label, .. } => {
                    let (title, shortcut_letter) = format_menu_label(label);
                    entries.push(MenuBarEntry {
                        bounds: Rect::ZERO,
                        title,
                        shortcut_letter,
                        title_offset: Vec2::ZERO,
                        index,
                    });
                }
                _ => {}
            }
        }

        ElementBuilder::new_cyclic(|weak_this| MenuBar {
            weak_this,
            entries,
            nodes,
            index_to_id,
            highlighted: None,
            menu: None,
        })
    }
}

const MENU_BAR_LEFT_PADDING: f64 = 4.0;
const MENU_BAR_BASELINE: f64 = 16.0;
const MENU_BAR_ITEM_PADDING: f64 = 4.0;

impl<ID: 'static + Clone> MenuBar<ID> {
    fn hit_test_bar(&self, local_pos: Vec2) -> Option<usize> {
        for (i, entry) in self.entries.iter().enumerate() {
            if entry.bounds.contains(local_pos.to_point()) {
                return Some(i);
            }
        }
        None
    }

    /// Opens the menu for the given entry index in the menu bar.
    fn open_menu(&mut self, cx: &TreeCtx, entry_index: usize) {

        let Some(nodes) = self.nodes.child_item_range(self.entries[entry_index].index) else {
            // no items in menu
            return;
        };

        let entry_bounds_screen = cx.map_rect_to_monitor(self.entries[entry_index].bounds);
        let owner = cx.get_platform_window();
        let popup = MenuBase::new(owner.clone(), nodes)
            .set_focus()
            .open_around(entry_bounds_screen, PopupPlacement::BottomThenUp);

        // convert internal events to typed events
        subscribe_global::<InternalMenuEntryActivated>({
            let popup_handle = popup.handle();
            let weak_this = self.weak_this.clone();
            move |InternalMenuEntryActivated { index }| {
                // unsubscribe when the menu is closed or the menu bar is dropped
                let Some(this) = weak_this.upgrade() else {
                    return false;
                };
                if !popup_handle.is_opened() {
                    return false;
                }

                if let Some(id) = this.borrow().index_to_id.get(index) {
                    emit_global(MenuEntryActivated(id.clone()));
                } else {
                    warn!("menu entry index {index} invalid for menu bar");
                }

                true
            }
        });

        self.menu = Some(popup);
        let weak_this = self.weak_this.clone();

        // TODO spawn a task that closes the menu when the window dismisses the popup
        //
        /*spawn(async move {
            owner.popup_cancelled().await;
            if let Some(this) = weak_this.upgrade() {
                this.invoke(|this, _| {
                    this.menu = None;
                });
            }
        });*/
    }
}

impl<ID: 'static + Clone> Element for MenuBar<ID> {
    fn measure(&mut self, _cx: &TreeCtx, layout_input: &LayoutInput) -> Size {
        let width = layout_input.width.available().unwrap_or_default();
        let height = 24.0;
        Size { width, height }
    }

    fn layout(&mut self, _cx: &TreeCtx, size: Size) -> LayoutOutput {
        let mut x = MENU_BAR_LEFT_PADDING;
        for entry in self.entries.iter_mut() {
            entry.title.layout(f64::INFINITY);

            let width = entry.title.size().width.round() + 2.0 * MENU_BAR_ITEM_PADDING;
            entry.bounds.x0 = x;
            entry.bounds.x1 = x + width;
            entry.bounds.y0 = 0.0;
            entry.bounds.y1 = size.height;

            entry.title_offset.x = x + MENU_BAR_ITEM_PADDING;
            entry.title_offset.y = MENU_BAR_BASELINE - entry.title.baseline();
            x += width;
        }
        LayoutOutput {
            width: size.width,
            height: size.height,
            baseline: Some(MENU_BAR_BASELINE),
        }
    }

    fn hit_test(&self, ctx: &mut HitTestCtx, point: Point) -> bool {
        ctx.bounds.contains(point)
    }

    fn paint(&mut self, ectx: &TreeCtx, ctx: &mut PaintCtx) {
        let bounds = ectx.bounds();
        for entry in self.entries.iter_mut() {
            let text_offset = point(bounds.x0 + entry.title_offset.x, bounds.y0 + entry.title_offset.y);
            ctx.draw_text_layout(text_offset, &entry.title);
        }
    }

    fn event(&mut self, ctx: &TreeCtx, event: &mut Event) {
        match event {
            Event::PointerDown(event) => {
                let local_pos = event.position - ctx.bounds().origin();
                if let Some(index) = self.hit_test_bar(local_pos) {
                    self.highlighted = Some(index);
                    self.open_menu(ctx, index);
                    ctx.mark_needs_paint();
                }
            }
            Event::PointerMove(event) => {
                let local_pos = event.position - ctx.bounds().origin();
                if let Some(index) = self.hit_test_bar(local_pos) {
                    if self.highlighted != Some(index) {
                        self.highlighted = Some(index);
                        if self.menu.is_some() {
                            self.open_menu(ctx, index);
                        }
                        ctx.mark_needs_paint();
                    }
                }
            }
            _ => {
                // TODO: close menu on escape or unfocus
            }
        }
    }
}
