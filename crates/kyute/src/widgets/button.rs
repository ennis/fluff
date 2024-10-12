use std::rc::Rc;

use kurbo::Vec2;
use smallvec::smallvec;

use crate::{Color, text};
use crate::drawing::BoxShadow;
use crate::text::TextStyle;
use crate::theme::DARK_THEME;
use crate::widgets::frame::{Frame, FrameStyle, FrameStyleOverride, InteractState};
use crate::widgets::text::Text;

fn button_style() -> FrameStyle {
    thread_local! {
        pub static BUTTON_STYLE: FrameStyle =
        FrameStyle {
            layout: Default::default(),
            border_left: Default::default(),
            border_right: Default::default(),
            border_top: Default::default(),
            border_bottom: Default::default(),
            border_color: Color::from_hex("4c3e0a"),
            border_radius: 8.0.into(),
            background_color: Color::from_hex("211e13"),
            shadows: smallvec![
                    BoxShadow {
                        color: Color::from_hex("4c3e0a"),
                        offset: Vec2::new(0.0, 1.0),
                        blur: 2.0,
                        spread: -1.0,
                        inset: false,
                    },
                ],
            overrides: smallvec![FrameStyleOverride {
                state: InteractState::ACTIVE,
                background_color: Some(Color::from_hex("4c3e0a")),
                ..Default::default()
            },
                FrameStyleOverride {
                state: InteractState::FOCUSED,
                border_color: Some(DARK_THEME.accent_color),
                ..Default::default()
                },
                FrameStyleOverride {
                state: InteractState::HOVERED,
                    background_color: Some(Color::from_hex("474029")),
                ..Default::default()
                },
            ],
        };
    }
    BUTTON_STYLE.with(|s| s.clone())
}

pub fn button(label: impl Into<String>) -> Rc<Frame> {
    let label = label.into();
    let theme = &DARK_THEME;
    let text_style = TextStyle::new()
        .font_size(theme.font_size as f32)
        .font_family(theme.font_family)
        .color(Color::from_hex("ffe580"));
    //let text = AttributedStr { str: &label, style:& text_style };
    let text = Text::new(text!( style(text_style) "{label}" ));
    let frame = Frame::new(button_style());
    frame.add_child(&text);
    frame
}
