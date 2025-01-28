#![feature(unique_rc_arc)]
#![feature(arbitrary_self_types)]

mod app_globals;
pub mod application;
mod backend;
pub mod compositor;
pub mod drawing;
pub mod element;
mod element_state;
pub mod event;
pub mod handler;
pub mod layout;
pub mod model;
pub mod notifier;
mod paint_ctx;
pub mod style;
pub mod text;
pub mod theme;
pub mod widgets;
pub mod window;

// reexports
pub use app_globals::AppGlobals;
pub use element::Element;
pub use element_state::ElementState;
pub use event::Event;
pub use kurbo::{self, Point, Rect, Size};
pub use kyute_common::Color;
pub use notifier::Notifier;
pub use paint_ctx::PaintCtx;
pub use skia_safe;
pub use window::{Window, WindowOptions};

pub use tokio::select;

#[doc(hidden)]
pub use inventory;
