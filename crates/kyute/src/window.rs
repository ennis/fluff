//! Window management.
//!
//! `Window` manages an operating system window that hosts a tree of `Visual` elements.
//! It is responsible for translating window events from winit into `Events` that are dispatched to the `Visual` tree.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::rc::{Rc, Weak};
use std::sync::OnceLock;
use std::time::Instant;

use keyboard_types::{Key, KeyboardEvent};
use kurbo::{Point, Rect, Size};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use skia_safe::font::Edging;
use skia_safe::{Font, FontMgr, FontStyle, Typeface};
use tracing::warn;
use winit::event::{DeviceId, ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::KeyLocation;
use winit::monitor::MonitorHandle;

use crate::application::WindowHandler;
use crate::compositor::{ColorType, Composition, CompositionBuilder};
use crate::drawing::ToSkia;
use crate::element::{dispatch_event, get_keyboard_focus, ChangeFlags, ElementAny, FocusedElement, HitTestCtx, IntoElementAny, WeakElementAny};
use crate::event::{
    key_event_to_key_code, Event, PointerButton, PointerButtons, PointerEvent, ScrollDelta, WheelEvent,
};
use crate::layout::{LayoutInput, SizeConstraint};
use crate::paint_ctx::paint_root_element;
use crate::{app_backend, application, double_click_time, platform, Color, EventSource};

fn draw_crosshair(canvas: &skia_safe::Canvas, pos: Point) {
    let mut paint = skia_safe::Paint::default();
    paint.set_color(skia_safe::Color::WHITE);
    paint.set_anti_alias(true);
    paint.set_stroke_width(1.0);
    paint.set_style(skia_safe::paint::Style::Stroke);

    let size = 100.;
    let x = pos.x as f32 + 0.5;
    let y = pos.y as f32 + 0.5;
    canvas.draw_line((x - size, y), (x + size, y), &paint);
    canvas.draw_line((x, y - size), (x, y + size), &paint);
    // draw a circle around the crosshair
    canvas.draw_circle((x, y), 10.0, &paint);
}

fn draw_text_blob(canvas: &skia_safe::Canvas, str: &str, size: Size) {
    // draw a text blob in the middle of the window
    let mut paint = skia_safe::Paint::default();
    paint.set_color(skia_safe::Color::WHITE);
    paint.set_anti_alias(true);
    paint.set_stroke_width(1.0);
    paint.set_style(skia_safe::paint::Style::Fill);
    let mut font = Font::from_typeface(default_typeface(), 12.0);
    font.set_subpixel(true);
    font.set_edging(Edging::SubpixelAntiAlias);
    let text_blob = skia_safe::TextBlob::from_str(str, &font).unwrap();
    canvas.draw_text_blob(text_blob, (0.0, size.height as f32 - 16.0), &paint);
}

static DEFAULT_TYPEFACE: OnceLock<Typeface> = OnceLock::new();

pub fn default_typeface() -> Typeface {
    DEFAULT_TYPEFACE
        .get_or_init(|| {
            let font_mgr = FontMgr::new();
            font_mgr
                .match_family_style("Inter Display", FontStyle::default())
                .unwrap()
        })
        .clone()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusChanged(pub bool);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseRequested;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resized(pub Size);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupCancelled;

/// Stores information about the last click (for double-click handling)
#[derive(Clone, Debug)]
struct LastClick {
    device_id: DeviceId,
    button: PointerButton,
    position: Point,
    time: Instant,
    repeat_count: u32,
}

#[derive(Default)]
struct InputState {
    /// Modifier state. Tracked here because winit doesn't want to give it to us in events.
    modifiers: keyboard_types::Modifiers,
    /// Pointer button state.
    pointer_buttons: PointerButtons,
    last_click: Option<LastClick>,
    // Result of the previous hit-test
    last_innermost_hit: Option<WeakElementAny>,
    last_hits: BTreeSet<WeakElementAny>,
    //prev_hit_test_result: Vec<HitTestEntry>,
}

/// How to place a popup window (like a context menu) relative to an anchor rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupPlacement {
    /// Tries to place the popup window to the right of the anchor rectangle, with its top
    /// aligned with the top of the anchor rectangle.
    ///
    /// If there's not enough space, then it will fall back to placing the popup to the left
    /// of the anchor rectangle.
    RightThenLeft,
    /// Tries to place the popup window to the right of the anchor rectangle. If not possible,
    /// then the popup may overlap the anchor rectangle.
    RightOrOverlap,
    BottomThenUp,
}

fn place_popup_right_or_left(monitor: Size, rect: Rect, menu: Size, allow_x_overlap: bool) -> Point {
    let mut x = rect.x1;
    if x + menu.width > monitor.width {
        // overflows on the right
        if allow_x_overlap {
            // shift menu to the left
            x = rect.x1 - (x + menu.width - monitor.width);
        } else {
            // place menu to the left
            x = rect.x0 - menu.width;
        }
        x = x.max(0.0);
    }
    let mut y = rect.y0;
    if y + menu.height > monitor.height {
        // overflows on the bottom
        y = rect.y0 - menu.height;
        y = y.max(0.0);
    }
    Point { x, y }
}

fn place_popup_bottom_or_top(monitor: Size, rect: Rect, menu: Size) -> Point {
    let mut y = rect.y1;
    if y + menu.height > monitor.height {
        // overflows on the bottom, fallback to positioning above the menu bar
        y = rect.y0 - menu.height;
        y = y.max(0.0);
    }
    let mut x = rect.x0;
    if x + menu.width > monitor.width {
        // overflows on the right, position menu to the left
        x = rect.x0 - menu.width;
        x = x.max(0.0);
    }
    Point { x, y }
}

/// Places a popup window relative to an anchor rectangle.
///
/// # Arguments
/// * `monitor` - The monitor on which the popup is going to be placed.
///               Can be `None` if not known, in which case the popup may be placed outside the monitor.
/// * `popup_size` - The size of the popup window.
/// * `anchor_rect` - The rectangle that the popup should be placed relative to, in monitor coordinates.
/// * `popup_placement` - How to place the popup window.
pub fn place_popup(
    monitor: Option<Monitor>,
    popup_size: Size,
    anchor_rect: Rect,
    popup_placement: PopupPlacement,
) -> Point {
    let monitor_size = if let Some(ref monitor) = monitor {
        monitor.logical_size()
    } else {
        Size::new(f64::INFINITY, f64::INFINITY)
    };
    match popup_placement {
        PopupPlacement::RightThenLeft => place_popup_right_or_left(monitor_size, anchor_rect, popup_size, false),
        PopupPlacement::RightOrOverlap => place_popup_right_or_left(monitor_size, anchor_rect, popup_size, true),
        PopupPlacement::BottomThenUp => place_popup_bottom_or_top(monitor_size, anchor_rect, popup_size),
    }
}

pub(crate) struct WindowInner {
    weak_this: Weak<WindowInner>,
    root: ElementAny,
    /// Previous compositor layers.
    composition: RefCell<Option<Composition>>,
    window: platform::Window,
    hidden_before_first_draw: Cell<bool>,
    cursor_pos: Cell<Point>,
    last_physical_size: Cell<Size>,
    input_state: RefCell<InputState>,
    /// The widget that is currently capturing pointer events.
    pointer_capture: RefCell<Option<WeakElementAny>>,
    background: Cell<Color>,
    active_popup: RefCell<Option<Weak<WindowInner>>>,
    // DEBUGGING
    last_kb_event: RefCell<Option<KeyboardEvent>>,
    /// Flag indicating that the element tree of this window should be laid out again.
    needs_layout: Cell<bool>,
}

impl WindowInner {
    fn set_pointer_capture(&self, element: WeakElementAny) {
        //if let Some(element) = element.upgrade() {
        //    self.check_belongs_to_window(element.node());
        //}
        //eprintln!("set_pointer_capture {}", element.name());
        self.pointer_capture.replace(Some(element));
    }

    fn map_to_screen(&self, point: Point) -> Point {
        // FIXME: this assumes that the scale factor of the window is the same
        //        as the scale factor of the monitor. I'm not sure if this is always the case.
        let window_pos = self
            .window
            .inner_position()
            .expect("failed to get window position")
            .to_logical::<f64>(self.window.scale_factor());

        Point {
            x: point.x + window_pos.x,
            y: point.y + window_pos.y,
        }
    }

    /// Dispatches a keyboard event in the UI tree.
    ///
    /// Currently, it just sends it to the focused element, or drops it if there's no focused element.
    fn dispatch_keyboard_event(&self, mut event: Event) {
        // Send the event to the element that has the focus.

        // FIXME: we assume that the element that has the focus is contained within this window's
        //        element tree. This is *usually* the case because `set_keyboard_focus` is typically called
        //        in response to a pointer event that activates the target window. However, this
        //        is not a guarantee (the focus could be set programmatically).
        //        We should probably add a check to see if the window of the focus target is our window.
        //        Also, `set_keyboard_focus` should also focus the window.
        // FIXME: actually the focused element may be in another window altogether, in case there's
        //        a non-focusable popup window.
        if let Some(FocusedElement { element }) = get_keyboard_focus() {
            if let Some(element) = element.upgrade() {
                dispatch_event(element, &mut event, true);
            }
        }

        // TODO do this only if the event was not consumed

        // Handle tab navigation
        match event {
            Event::KeyDown(ke) if ke.key == Key::Tab => {
                todo!("tab navigation");
                /*if let Some(focus) = self.focus.upgrade() {
                    // Go to next focusable element
                    if let Some(next_focus) = focus.next_focusable_element() {
                        self.set_focus(next_focus.weak());
                    } else if let Some(next_focus) = self.root.next_focusable_element() {
                        // cycle back to the first focusable element
                        self.set_focus(next_focus.weak());
                    } else {
                        // no focusable elements
                        self.set_focus(WeakElement::new());
                    }
                }*/
            }
            _ => {}
        }
    }

    /// Dispatches a pointer event in the UI tree.
    ///
    /// It first determines the target of the event (i.e. either the pointer-capturing element or
    /// the deepest element that passes the hit-test), then propagates the event to the target with `send_event`.
    ///
    /// TODO It should also handle focus and hover update events (FocusGained/Lost, PointerOver/Out).
    ///
    /// # Return value
    ///
    /// Returns true if the app logic should re-run in response of the event.
    fn dispatch_pointer_event(&self, mut event: Event, hit_position: Point) {
        debug_assert!(event.pointer_event().is_some(), "event must be a pointer event");

        let mut input_state = self.input_state.borrow_mut();

        let mut hit_test_ctx = HitTestCtx::new();
        self.root.hit_test(&mut hit_test_ctx, hit_position);
        let hits = hit_test_ctx.hits;
        let innermost_hit = hits.first().cloned();
        let is_pointer_up = matches!(event, Event::PointerUp(_));

        // If something is grabbing the pointer, then the event is delivered to that element;
        // otherwise it is delivered to the innermost widget that passes the hit-test.
        let has_pointer_capture = self.pointer_capture.borrow().is_some();
        let target = self.pointer_capture.borrow().clone().or(innermost_hit.clone());

        if let Some(target) = target {
            if let Some(target) = target.upgrade() {
                event.pointer_event_mut().unwrap().request_capture = has_pointer_capture;
                dispatch_event(target, &mut event, true);
            }
        }

        // release pointer capture automatically on pointer up
        if is_pointer_up {
            self.pointer_capture.replace(None);
        }

        // Now send pointerleave/pointerenter/pointerout/pointerover events
        let p = PointerEvent {
            position: hit_position,
            modifiers: input_state.modifiers,
            buttons: input_state.pointer_buttons,
            button: None,
            repeat_count: 0,
            request_capture: false,
        };

        // convert hits to set
        let hits_set = BTreeSet::from_iter(hits);

        let hit_changed = input_state.last_innermost_hit != innermost_hit;

        // send pointerout
        if hit_changed {
            if let Some(ref out) = input_state.last_innermost_hit {
                if let Some(out) = out.upgrade() {
                    dispatch_event(out, &mut Event::PointerOut(p), true);
                }
            }
        }
        // send pointerleave
        let leaving = input_state.last_hits.difference(&hits_set);
        for v in leaving {
            if let Some(v) = v.upgrade() {
                dispatch_event(v, &mut Event::PointerLeave(p), false);
            }
        }

        // send pointerover
        if hit_changed {
            if let Some(ref over) = innermost_hit {
                if let Some(over) = over.upgrade() {
                    dispatch_event(over, &mut Event::PointerOver(p), true);
                }
            }
        }

        // send pointerenter
        let entering = hits_set.difference(&input_state.last_hits);
        for v in entering {
            if let Some(v) = v.upgrade() {
                dispatch_event(v, &mut Event::PointerEnter(p), false);
            }
        }

        // update last hits
        input_state.last_hits = hits_set;
        input_state.last_innermost_hit = innermost_hit;
    }

    /// Converts a winit mouse event to an Event, and update internal state.
    fn convert_mouse_input(&self, device_id: DeviceId, button: MouseButton, state: ElementState) -> Option<Event> {
        let mut input_state = self.input_state.borrow_mut();
        let button = match button {
            MouseButton::Left => PointerButton::LEFT,
            MouseButton::Right => PointerButton::RIGHT,
            MouseButton::Middle => PointerButton::MIDDLE,
            MouseButton::Back => PointerButton::X1,
            MouseButton::Forward => PointerButton::X2,
            MouseButton::Other(_) => {
                // FIXME ignore extended buttons for now, but they should really be propagated as well
                return None;
            }
        };
        // update tracked state
        if state.is_pressed() {
            input_state.pointer_buttons.set(button);
        } else {
            input_state.pointer_buttons.reset(button);
        }
        let click_time = Instant::now();

        // determine the repeat count (double-click, triple-click, etc.) for button down event
        let repeat_count = match &mut input_state.last_click {
            Some(ref mut last)
                if last.device_id == device_id
                    && last.button == button
                    && last.position == self.cursor_pos.get()
                    && (click_time - last.time) < double_click_time() =>
            {
                // same device, button, position, and within the platform specified double-click time
                if state.is_pressed() {
                    last.repeat_count += 1;
                    last.repeat_count
                } else {
                    // no repeat for release events (although that could be possible?)
                    1
                }
            }
            other => {
                // no match, reset
                if state.is_pressed() {
                    *other = Some(LastClick {
                        device_id,
                        button,
                        position: self.cursor_pos.get(),
                        time: click_time,
                        repeat_count: 1,
                    });
                } else {
                    *other = None;
                };
                1
            }
        };
        let pe = PointerEvent {
            position: self.cursor_pos.get(),
            modifiers: input_state.modifiers,
            buttons: input_state.pointer_buttons,
            button: Some(button),
            repeat_count: repeat_count as u8,
            request_capture: false,
        };

        let event = if state.is_pressed() {
            Event::PointerDown(pe)
        } else {
            Event::PointerUp(pe)
        };

        Some(event)
    }

    fn convert_keyboard_input(&self, key_event: &KeyEvent) -> Event {
        let input = &mut *self.input_state.borrow_mut();
        let (key, code) = key_event_to_key_code(&key_event);
        // update modifiers
        match (&key, key_event.state) {
            (Key::Shift, ElementState::Pressed) => input.modifiers.insert(keyboard_types::Modifiers::SHIFT),
            (Key::Shift, ElementState::Released) => input.modifiers.remove(keyboard_types::Modifiers::SHIFT),
            (Key::Control, ElementState::Pressed) => input.modifiers.insert(keyboard_types::Modifiers::CONTROL),
            (Key::Control, ElementState::Released) => input.modifiers.remove(keyboard_types::Modifiers::CONTROL),
            (Key::Alt, ElementState::Pressed) => input.modifiers.insert(keyboard_types::Modifiers::ALT),
            (Key::Alt, ElementState::Released) => input.modifiers.remove(keyboard_types::Modifiers::ALT),
            (Key::Meta, ElementState::Pressed) => input.modifiers.insert(keyboard_types::Modifiers::META),
            (Key::Meta, ElementState::Released) => input.modifiers.remove(keyboard_types::Modifiers::META),
            _ => {}
        }

        let ke = KeyboardEvent {
            state: match key_event.state {
                ElementState::Pressed => keyboard_types::KeyState::Down,
                ElementState::Released => keyboard_types::KeyState::Up,
            },
            key,
            code,
            location: match key_event.location {
                KeyLocation::Standard => keyboard_types::Location::Standard,
                KeyLocation::Left => keyboard_types::Location::Left,
                KeyLocation::Right => keyboard_types::Location::Right,
                KeyLocation::Numpad => keyboard_types::Location::Numpad,
            },
            modifiers: input.modifiers,
            repeat: key_event.repeat,
            is_composing: false,
        };

        self.last_kb_event.replace(Some(ke.clone()));
        //eprintln!("[{:?}] key={:?}, code={:?}, modifiers={:?}", self.window.id(), key, code, input.modifiers);

        match ke.state {
            keyboard_types::KeyState::Down => Event::KeyDown(ke),
            keyboard_types::KeyState::Up => Event::KeyUp(ke),
        }
    }

    fn redirect_event_to_popup(&self, _popup: &WindowInner, event: &WindowEvent) -> Option<WindowEvent> {
        // strategy: translate the event so that it appears to come from the popup window,
        // then directly invoke `dispatch_winit_input_event` on the popup window
        /*let _self_client_area = {
            let pos = self.window.inner_position().unwrap().cast();
            let size = self.window.inner_size().cast();
            Rect::from_origin_size(Point::new(pos.x, pos.y), Size::new(size.width, size.height))
        };

        let _popup_client_area = {
            let pos = popup.window.inner_position().unwrap().cast();
            let size = popup.window.inner_size().cast();
            Rect::from_origin_size(Point::new(pos.x, pos.y), Size::new(size.width, size.height))
        };*/

        let translated_event = event.clone();
        let redirect;
        match translated_event {
            /*WindowEvent::CursorMoved { ref mut position, .. } => {
                let pos = Point::new(position.x, position.y);
                // FIXME: multiple desktops?
                let desktop_pos = self_client_area.origin() + pos.to_vec2();
                if popup_client_area.contains(desktop_pos) {
                    position.x = desktop_pos.x - popup_client_area.x0;
                    position.y = desktop_pos.y - popup_client_area.y0;
                    redirect = true;
                }
            }*/
            WindowEvent::KeyboardInput { .. } => {
                // no translation necessary
                redirect = true;
            }
            _ => {
                redirect = false;
            }
        }

        if redirect {
            Some(translated_event)
        } else {
            None
        }
    }

    pub(crate) fn set_popup(&self, window: &Window) {
        self.active_popup.replace(Some(Rc::downgrade(&window.shared)));
    }

    fn mark_needs_layout(&self) {
        self.needs_layout.set(true);
        self.window.request_redraw();
    }

    /*fn monitor(&self) -> Monitor {
        Monitor(self.window.current_monitor().expect("could not retrieve monitor for window"))
    }*/

    fn mark_needs_paint(&self) {
        //self.window.request_redraw();
    }

    /// Converts & dispatches a winit window event.
    fn dispatch_window_event(&self, event: &WindowEvent) {
        // First, redirect the input event to the popup window if there is one.
        let popup = self.active_popup.borrow().clone();
        if let Some(popup) = popup {
            if let Some(popup) = popup.upgrade() {
                if let Some(redirected_event) = self.redirect_event_to_popup(&popup, event) {
                    popup.dispatch_window_event(&redirected_event);
                    return;
                }
            } else {
                self.active_popup.replace(None);
            }
        }

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let scale_factor = self.window.scale_factor();
                let pos = Point::new(position.x / scale_factor, position.y / scale_factor);
                //eprintln!("[{:?}] CursorMoved: {:?}", self.window.id(), pos);
                self.cursor_pos.set(pos);
                let modifiers = self.input_state.borrow().modifiers;
                let buttons = self.input_state.borrow().pointer_buttons;
                self.dispatch_pointer_event(
                    Event::PointerMove(PointerEvent {
                        position: pos,
                        modifiers,
                        buttons,
                        button: None,
                        repeat_count: 0,
                        request_capture: false,
                    }),
                    pos,
                );
                // force a redraw for the debug crosshair
                //self.window.request_redraw();
            }
            WindowEvent::Touch(touch) => {
                self.cursor_pos.set(Point::new(touch.location.x, touch.location.y));
                // force a redraw for the debug crosshair
                //self.window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let converted_event = self.convert_keyboard_input(event);
                self.dispatch_keyboard_event(converted_event);
                // for the debugging overlay
                //self.window.request_redraw();
            }
            WindowEvent::MouseInput {
                button,
                state,
                device_id,
            } => {
                if let Some(event) = self.convert_mouse_input(*device_id, *button, *state) {
                    self.dispatch_pointer_event(event, self.cursor_pos.get());
                }
                //self.weak_this.emit(PopupCancelled);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(FocusedElement { element }) = get_keyboard_focus() {
                    if let Some(element) = element.upgrade() {
                        dispatch_event(
                            element,
                            &mut Event::Wheel(WheelEvent {
                                delta: match *delta {
                                    MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines {
                                        x: x as f64,
                                        y: y as f64,
                                    },
                                    MouseScrollDelta::PixelDelta(pos) => ScrollDelta::Pixels { x: pos.x, y: pos.y },
                                },
                            }),
                            true,
                        );
                    }
                }
            }
            WindowEvent::CloseRequested => {
                self.weak_this.emit(CloseRequested);
            }
            WindowEvent::Resized(size) => {
                let sizef = Size::new(size.width as f64, size.height as f64);
                self.weak_this.emit(Resized(sizef));
                if size.width != 0 && size.height != 0 {
                    // resize the compositor layer
                    //self.
                    //self.layer.resize(sizef);
                    self.window.request_redraw();
                }
                self.needs_layout.set(true);
            }
            WindowEvent::Focused(focused) => {
                eprintln!("[window@{:?}] Focused: {:?}", self.window.id(), focused);
                self.weak_this.emit(FocusChanged(*focused));
                // FIXME: this could be a global event instead
                self.weak_this.emit(PopupCancelled);
            }
            WindowEvent::RedrawRequested => {
                //eprintln!("[{:?}] RedrawRequested", self.window.id());
                self.do_redraw();
            }
            _ => {}
        }
    }

    fn do_redraw(&self) {
        let scale_factor = self.window.scale_factor();
        let physical_size = self.window.inner_size();
        if physical_size.width == 0 || physical_size.height == 0 {
            return;
        }
        let size = physical_size.to_logical(scale_factor);
        let physical_size = Size::new(physical_size.width as f64, physical_size.height as f64);
        let size = Size::new(size.width, size.height);

        if physical_size != self.last_physical_size.get() {
            self.last_physical_size.set(physical_size);
            //self.layer.set_surface_size(physical_size);
        }

        // TODO: a more principled way to determine if we need to redraw
        if !self.root.0.ctx.change_flags.get().contains(ChangeFlags::PAINT) && !self.needs_layout.get() {
            return;
        }

        if self.needs_layout.replace(false) {
            let size = self.root.measure_root(&LayoutInput {
                parent_width: Some(size.width),
                parent_height: Some(size.height),
                width: SizeConstraint::Available(size.width),
                height: SizeConstraint::Available(size.height),
            });
            let _geom = self.root.layout_root(size);
        }


        {
            let mut composition_builder =
                CompositionBuilder::new(scale_factor, size.to_rect(), self.composition.take());
            composition_builder.canvas().clear(self.background.get().to_skia());

            paint_root_element(&self.root, &mut composition_builder);

            // **** DEBUGGING ****
            draw_crosshair(
                composition_builder.picture_recorder().recording_canvas().unwrap(),
                self.cursor_pos.get(),
            );

            if let Some(event) = &*self.last_kb_event.borrow() {
                //draw_text_blob(
                //    skia_surface.canvas(),
                //    &format!("{:?} ({:?}) +{:?}", event.key, event.code, event.modifiers),
                //    size,
                //);
            }

            let composition = composition_builder.finish();
            composition.render_to_window(&self.window);
            self.composition.replace(Some(composition));
        }

        // Nothing more to paint, release the surface.
        //
        // This flushes the skia command buffers, and presents the surface to the compositor.
        //drop(surface);

        // Windows are initially created hidden, and are only shown after the first frame is painted.
        // Now that we've rendered the first frame, we can reveal it.
        if self.hidden_before_first_draw.get() {
            self.hidden_before_first_draw.set(false);
            self.window.set_visible(true);
        }

        //self.clear_change_flags(ChangeFlags::PAINT);

        // Wait for the compositor to be ready to render another frame (this is to reduce latency)
        // FIXME: this assumes that there aren't any other windows waiting to be painted!
        //self.layer.wait_for_presentation();

        // latency test
        //sleep(std::time::Duration::from_millis(5));
    }
}

impl WindowHandler for WindowInner {
    fn event(&self, event: &WindowEvent) {
        self.dispatch_window_event(event);
    }

    fn redraw(&self) {
        self.do_redraw();
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
    }
}

pub struct Window {
    shared: Rc<WindowInner>,
}

impl EventSource for Window {
    fn as_weak(&self) -> Weak<dyn Any> {
        Rc::downgrade(&self.shared) as Weak<dyn Any>
    }
}

impl EventSource for Weak<WindowInner> {
    fn as_weak(&self) -> Weak<dyn Any> {
        self.clone() as Weak<dyn Any>
    }
}

/// A weak handle to a window.
///
/// This doesn't prevent the window from being dropped.
#[derive(Clone)]
pub struct WindowHandle {
    pub(crate) shared: Weak<WindowInner>,
}

/*
impl PartialEq for WeakWindow {
    fn eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.shared, &other.shared)
    }
}*/

impl Default for WindowHandle {
    fn default() -> Self {
        Self { shared: Weak::new() }
    }
}

impl WindowHandle {
    pub fn scale_factor(&self) -> f64 {
        if let Some(shared) = self.shared.upgrade() {
            shared.window.scale_factor()
        } else {
            warn!("scale_factor: window has been dropped");
            1.0
        }
    }

    /*pub fn request_repaint(&self) {
        if let Some(shared) = self.shared.upgrade() {
            shared.window.request_redraw();
        }
    }*/

    pub fn mark_needs_layout(&self) {
        if let Some(shared) = self.shared.upgrade() {
            shared.mark_needs_layout();
        } else {
            warn!("mark_needs_layout: window has been dropped");
        }
    }

    pub fn mark_needs_paint(&self) {
        if let Some(shared) = self.shared.upgrade() {
            shared.mark_needs_paint();
        } else {
            warn!("mark_needs_paint: window has been dropped");
        }
    }

    pub fn set_pointer_capture(&self, element: WeakElementAny) {
        if let Some(shared) = self.shared.upgrade() {
            shared.set_pointer_capture(element);
        } else {
            warn!("set_pointer_capture: window has been dropped");
        }
    }

    pub fn set_popup(&self, window: &Window) {
        if let Some(shared) = self.shared.upgrade() {
            shared.set_popup(window);
        } else {
            warn!("set_popup: window has been dropped");
        }
    }

    pub fn map_to_screen(&self, pos: Point) -> Point {
        self.shared
            .upgrade()
            .map(|shared| shared.map_to_screen(pos))
            .unwrap_or_default()
    }

    pub fn monitor(&self) -> Option<Monitor> {
        self.shared
            .upgrade()
            .and_then(|shared| shared.window.current_monitor())
            .map(Monitor)
    }

    pub fn raw_window_handle(&self) -> Option<RawWindowHandle> {
        Some(self.shared.upgrade()?.window.window_handle().ok()?.as_raw())
    }

    pub fn is_opened(&self) -> bool {
        self.shared.upgrade().is_some()
    }

    pub async fn popup_cancelled(&self) {
        self.shared.wait_event::<PopupCancelled>().await;
    }

    /*/// Returns a reference to the currently focused element.
    pub fn is_focused(&self, element: &Node) -> bool {
        self.shared
            .upgrade()
            .map(|shared| shared.is_focused(element))
            .unwrap_or(false)
    }*/
}

#[derive(Debug, Clone)]
pub struct Monitor(MonitorHandle);

impl Monitor {
    pub fn logical_size(&self) -> Size {
        let size = self.0.size().to_logical(self.0.scale_factor());
        Size::new(size.width, size.height)
    }
}

/// Describes the options for creating a new window.
pub struct WindowOptions<'a> {
    /// Initial title of the window.
    pub title: &'a str,
    /// Initial size of the window, in device-independent pixels (logical size).
    pub size: Size,
    /// Owner window handle (Windows only).
    // FIXME that's not really the parent but rather the "owner"
    pub parent: Option<RawWindowHandle>,
    /// Whether the window should have decorations (title bar, close button, etc).
    pub decorations: bool,
    /// Whether the window should be initially visible.
    pub visible: bool,
    /// Background color of the window.
    pub background: Color,
    /// Initial position of the window, in device-independent pixels.
    pub position: Option<Point>,
    /// If true, the window will not take focus when shown and when the user clicks on it.
    ///
    /// This is typically used for context menus.
    pub no_focus: bool,
}

impl<'a> Default for WindowOptions<'a> {
    fn default() -> Self {
        Self {
            title: "",
            size: Size::new(800.0, 600.0),
            parent: None,
            decorations: true,
            visible: true,
            background: Color::from_hex("#151515"),
            position: None,
            no_focus: false,
        }
    }
}

impl Window {
    pub fn new(options: &WindowOptions, root: impl IntoElementAny) -> Self {
        let platform_window = platform::Window::new(options);

        // Setup compositor layer
        // Get the physical size from the window
        let phy_size = platform_window.inner_size();
        let phy_size = Size::new(phy_size.width as f64, phy_size.height as f64);

        let window_id = platform_window.id();
        let shared = Rc::new_cyclic(|weak_this| WindowInner {
            weak_this: weak_this.clone(),
            root: root.into_root_element_any(WindowHandle {
                shared: weak_this.clone(),
            }),
            window: platform_window,
            hidden_before_first_draw: Cell::new(true),
            cursor_pos: Cell::new(Default::default()),
            last_physical_size: Cell::new(phy_size),
            input_state: Default::default(),
            pointer_capture: Default::default(),
            background: Cell::new(options.background),
            active_popup: RefCell::new(None),
            last_kb_event: RefCell::new(None),
            needs_layout: Cell::new(true),
            composition: RefCell::new(None),
        });

        application::register_window(window_id, shared.clone());
        Window { shared }
    }

    pub fn set_pointer_capture(&self, element: WeakElementAny) {
        self.shared.set_pointer_capture(element);
    }

    pub fn handle(&self) -> WindowHandle {
        WindowHandle {
            shared: Rc::downgrade(&self.shared),
        }
    }

    pub fn map_to_screen(&self, pos: Point) -> Point {
        self.shared.map_to_screen(pos)
    }

    pub fn monitor(&self) -> Option<Monitor> {
        self.shared.window.current_monitor().map(Monitor)
    }

    pub fn mark_needs_layout(&self) {
        self.shared.mark_needs_layout();
    }

    pub fn mark_needs_paint(&self) {
        self.shared.mark_needs_paint();
    }

    pub fn set_popup(&self, window: &Window) {
        self.shared.set_popup(window);
    }

    pub fn raw_window_handle(&self) -> RawWindowHandle {
        self.shared.window.window_handle().unwrap().as_raw()
    }

    /*pub fn on_close_requested(&self, f: impl Fn() + 'static) {
        self.shared.close_requested.watch(move |_| f());
    }

    pub fn on_resized(&self, f: impl Fn(Size) + 'static) {
        self.shared.resized.watch(f);
    }

    pub fn on_focus_changed(&self, f: impl Fn(bool) + 'static) {
        self.shared.focus_changed.watch(f);
    }*/

    pub async fn close_requested(&self) {
        self.wait_event::<CloseRequested>().await;
    }

    pub async fn resized(&self) -> Size {
        self.wait_event::<Resized>().await.0
    }

    pub async fn focus_changed(&self) -> bool {
        self.wait_event::<FocusChanged>().await.0
    }

    /// Hides the window.
    pub fn hide(&self) {
        self.shared.window.set_visible(false);
    }

    pub fn is_hidden(&self) -> bool {
        !self.shared.window.is_visible().unwrap()
    }
}
