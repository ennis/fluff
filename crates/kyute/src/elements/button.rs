use kurbo::{Insets, Vec2};

use crate::drawing::BoxShadow;
use crate::element_state::ElementState;
use crate::elements::flex::Flex;
use crate::elements::frame::{Frame, FrameStyle, FrameStyleOverride};
use crate::elements::HoveredEvent;
use crate::layout::SizeValue;
use crate::theme::DARK_THEME;
use crate::{text, Color, NodeBuilder};

fn button_style() -> FrameStyle {
    thread_local! {
        pub static BUTTON_STYLE: FrameStyle =
        FrameStyle {
            border_size: Insets::uniform(1.0),
            border_color: Color::from_hex("4c3e0a"),
            border_radius: 5.0.into(),
            background_color: Color::from_hex("211e13"),
            shadows: vec![
                    BoxShadow {
                        color: Color::from_hex("4c3e0a"),
                        offset: Vec2::new(0.0, 1.0),
                        blur: 2.0,
                        spread: -1.0,
                        inset: false,
                    },
                ],
            overrides: vec![FrameStyleOverride {
                state: ElementState::ACTIVE,
                background_color: Some(Color::from_hex("4c3e0a")),
                ..Default::default()
            },
                FrameStyleOverride {
                state: ElementState::FOCUSED,
                border_color: Some(DARK_THEME.accent_color),
                ..Default::default()
                },
                FrameStyleOverride {
                state: ElementState::HOVERED,
                    background_color: Some(Color::from_hex("474029")),
                ..Default::default()
                },
            ],
        };
    }
    BUTTON_STYLE.with(|s| s.clone())
}

pub fn button(label: impl Into<String>) -> NodeBuilder<Frame> {
    let label = label.into();
    let theme = &DARK_THEME;

    Frame::new()
        .style(button_style())
        .padding(4.0)
        .width(SizeValue::MaxContent)
        .min_width(80)
        .content(Flex::row().gaps(SizeValue::Stretch, 0, SizeValue::Stretch).child(
            text!( FontSize(theme.font_size) FontFamily(theme.font_family) Color(Color::from_hex("ffe580")) "{label}" ),
        ))
        .on(|button, cx, HoveredEvent(hovered)| {
            if *hovered {
                button.set_background_color(cx, Color::from_hex("474029"));
            } else {
                button.set_background_color(cx, Color::from_hex("211e13"));
            }
        })
}
