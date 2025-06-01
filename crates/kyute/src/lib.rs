#![feature(unique_rc_arc)]
#![feature(arbitrary_self_types)]
#![feature(default_field_values)]


#![expect(unused, reason = "noisy")]

mod app_globals;
pub mod application;
pub mod compositor;
pub mod drawing;
mod element_state;
pub mod elements;
pub mod input_event;
pub mod layout;
pub mod event;
mod paint_ctx;
pub mod platform;
pub mod text;
pub mod theme;
mod util;
pub mod window;

mod node;
mod element;
mod focus;
mod debug;
//pub mod model;

// reexports
pub use app_globals::{app_backend, caret_blink_time, double_click_time, init_application, teardown_application};
pub use element::{Element};
pub use node::{NodeBuilder, HitTestCtx, IntoNode, NodeCtx, RcNode, WeakNode, NodeData, WeakDynNode, RcDynNode};
pub use element_state::ElementState;
pub use input_event::Event;
pub use kurbo::{self, Point, Rect, Size};
pub use kyute_common::Color;
pub use event::EventSource;
pub use paint_ctx::PaintCtx;
pub use skia_safe;
pub use window::{Window};
pub use layout::{Measurement, LayoutInput};

pub use futures::future::AbortHandle;
pub use tokio::select;
