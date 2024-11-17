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
mod reactive;
//mod skia_backend;
pub mod style;
pub mod text;
pub mod theme;
pub mod widgets;
pub mod window;
pub mod component;

// reexports
pub use app_globals::AppGlobals;
pub use kyute_common::Color;
pub use element::{Node, Element};
pub use component::{Component, ComponentPtr};
pub use event::Event;
pub use kurbo::{self, Point, Rect, Size};
pub use paint_ctx::PaintCtx;
pub use skia_safe;
pub use style::Style;
pub use window::{Window, WindowOptions};

pub use tokio::select;