//! Debug overlay.

use crate::compositor::{Composition, CompositionBuilder};
use crate::drawing::ToSkia;
use crate::node::Root;
use crate::platform::{PlatformWindowHandle, WindowHandler, WindowOptions};
use crate::text::{FontStretch, FontStyle, FontWeight, IntoTextLayout, TextStyle};
use crate::{text, RcDynNode};
use kurbo::{Point, Size};
use kyute_common::Color;
use std::borrow::Cow;
use std::cell::{OnceCell, RefCell, RefMut};
use std::collections::{HashMap, VecDeque};
use winit::event::WindowEvent;

const LINE_HEIGHT: f64 = 13.0;
const TEXT: &TextStyle = &TextStyle {
    font_family: Cow::Borrowed("monospace"),
    font_size: 12.0,
    font_weight: FontWeight::NORMAL,
    font_style: FontStyle::Normal,
    font_stretch: FontStretch::NORMAL,
    color: Color::from_rgb_u8(255, 255, 255),
    underline: false,
};

struct DebugState {
    debug_window: PlatformWindowHandle,
    prev_comp: Option<Composition>,
    window_debug_infos: HashMap<PlatformWindowHandle, WindowDebugInfo>,
}

impl DebugState {
    fn draw_text(&self, comp: &mut CompositionBuilder, position: Point, text: impl IntoTextLayout) {
        let mut text = text.into_text_layout(&TEXT);
        text.layout(f64::INFINITY);
        text.paint(comp.canvas(), position);
    }

    fn paint_debug_window(&mut self) {
        let scale_factor = self.debug_window.scale_factor();
        let window_client_area = self.debug_window.client_area_size().to_rect();
        let mut comp = CompositionBuilder::new(scale_factor, window_client_area, self.prev_comp.take());
        self.paint(&mut comp);
        let comp = comp.finish();
        comp.render_to_window(&self.debug_window);
        self.prev_comp.replace(comp);
    }

    fn paint(&mut self, comp: &mut CompositionBuilder) {
        let canvas = comp.canvas();
        canvas.clear(Color::from_rgba_u8(0, 0, 0, 255).to_skia());
        self.draw_text(comp, Point::new(5.0, 5.0), text!["Debug Overlay"]);
    }

    fn print_node_hierarchy(&self, ui_tree: &Root, comp: &mut CompositionBuilder) {
        struct Entry {
            node: RcDynNode,
            depth: usize,
        }

        let mut to_visit = VecDeque::new();
        to_visit.push_back((0, ui_tree.root().clone()));
        while let Some((depth, node)) = to_visit.pop_front() {
            for child in node.children() {
                to_visit.push_back((depth + 1, child.clone()));
            }
        }
    }
}

impl WindowHandler for &RefCell<DebugState> {
    fn event(&self, window: PlatformWindowHandle, event: &WindowEvent) {
        match event {
            WindowEvent::RedrawRequested => {
                self.borrow_mut().paint_debug_window();
            }
            WindowEvent::CloseRequested => {
                window.close();
            }
            _ => {}
        }
    }

    fn redraw(&self, window: PlatformWindowHandle) {
    }

    fn request_redraw(&self, window: PlatformWindowHandle) {
    }
}

thread_local! {
    static DEBUG_STATE: OnceCell<&'static RefCell<DebugState>> = OnceCell::new();
}

const DEBUG_WINDOW_SIZE: Size = Size {
    width: 400.0,
    height: 300.0,
};

pub(crate) fn init_debug_state() {
    DEBUG_STATE.with(|h| {
        h.get_or_init(|| {
            let window = PlatformWindowHandle::new(&WindowOptions {
                title: "Debug",
                size: Some(DEBUG_WINDOW_SIZE),
                visible: true,
                background: Default::default(),
                position: None,
                center: false,
                kind: Default::default(),
                decorations: true,
                resizable: true,
                monitor: None,
            });
            let handler = &*Box::leak(Box::new(RefCell::new(DebugState {
                debug_window: window.clone(),
                prev_comp: None,
                window_debug_infos: Default::default(),
            })));
            window.set_handler(Box::new(handler));
            handler
        });
    })
}

fn write_debug_state() -> RefMut<'static, DebugState> {
    init_debug_state();
    let h = DEBUG_STATE.with(|h| *h.get().unwrap());
    h.borrow_mut()
}

pub(crate) struct WindowDebugInfo {
    handle: PlatformWindowHandle,
    root_node: RcDynNode,
}

pub(crate) fn set_window_debug_info(info: WindowDebugInfo) {
    let mut state = write_debug_state();
    state.window_debug_infos.insert(info.handle.clone(), info);
}

pub(crate) fn set_debug_focus(node: RcDynNode) {
    let mut state = write_debug_state();
    //
}