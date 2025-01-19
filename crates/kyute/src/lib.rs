#![feature(unique_rc_arc)]
#![feature(arbitrary_self_types)]

mod app_globals;
pub mod application;
mod backend;
pub mod compositor;
pub mod drawing;
pub mod element;
pub mod event;
pub mod handler;
pub mod layout;
mod paint_ctx;
pub mod style;
pub mod text;
pub mod theme;
pub mod widgets;
pub mod window;
pub mod notifier;
mod element_state;
pub mod model;

// reexports
pub use app_globals::AppGlobals;
pub use kyute_common::Color;
pub use element::{Element};
pub use event::Event;
pub use notifier::Notifier;
pub use kurbo::{self, Point, Rect, Size};
pub use paint_ctx::PaintCtx;
pub use skia_safe;
pub use style::Style;
pub use window::{Window, WindowOptions};
pub use element_state::ElementState;

pub use tokio::select;

#[doc(hidden)]
pub use inventory;