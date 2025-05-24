#![feature(unique_rc_arc)]
#![feature(arbitrary_self_types)]
#![feature(default_field_values)]

mod app_globals;
pub mod application;
pub mod compositor;
pub mod drawing;
pub mod element;
mod element_state;
pub mod elements;
pub mod input_event;
pub mod handler;
pub mod layout;
pub mod event;
pub mod notifier;
mod paint_ctx;
pub mod platform;
pub mod text;
pub mod theme;
mod util;
pub mod window;
//pub mod model;

// reexports
pub use app_globals::{app_backend, caret_blink_time, double_click_time, init_application, teardown_application};
pub use element::{Element, IntoElementAny, ElementBuilder};
pub use element_state::ElementState;
pub use input_event::Event;
pub use kurbo::{self, Point, Rect, Size};
pub use kyute_common::Color;
pub use event::EventSource;
pub use notifier::Notifier;
pub use paint_ctx::PaintCtx;
pub use skia_safe;
pub use window::{Window};

pub use futures::future::AbortHandle;
pub use tokio::select;
