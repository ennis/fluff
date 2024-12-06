use kurbo::Vec2;
use smallvec::smallvec;

use crate::drawing::BoxShadow;
use crate::element::RcElement;
use crate::text::TextStyle;
use crate::theme::DARK_THEME;
use crate::widgets::frame::{Frame, FrameLayout, FrameStyle, FrameStyleOverride, InteractState};
use crate::widgets::text::Text;
use crate::{text, Color};
use crate::layout::{Axis, FlexSize, SizeValue, Sizing};

fn button_style() -> FrameStyle {
    thread_local! {
        pub static BUTTON_STYLE: FrameStyle =
        FrameStyle {
            border_left: 1.0.into(),
            border_right: 1.0.into(),
            border_top: 1.0.into(),
            border_bottom: 1.0.into(),
            border_color: Color::from_hex("4c3e0a"),
            border_radius: 5.0.into(),
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

pub fn button(label: impl Into<String>) -> RcElement<Frame> {
    let label = label.into();
    let theme = &DARK_THEME;
    let text_style = TextStyle::new()
        .font_size(theme.font_size as f32)
        .font_family(theme.font_family)
        .color(Color::from_hex("ffe580"));

    let frame = Frame::new();
    frame.set_style(button_style());
    frame.set_padding(4.0.into());
    frame.set_layout(FrameLayout::Flex { direction: Axis::Horizontal, initial_gap: FlexSize { size: 0.0, flex: 1.0 }, final_gap: FlexSize { size: 0.0, flex: 1.0 }, gap: 0.0.into() });
    frame.set_width(SizeValue::MaxContent);
    frame.set_min_width(SizeValue::Fixed(80.0));
    frame.add_child(Text::new(text!( style(text_style) "{label}" )));
    frame
}
