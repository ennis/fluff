use kurbo::{Insets, Vec2};
use smallvec::smallvec;

use crate::drawing::BoxShadow;
use crate::element::{RcElement, WeakElement};
use crate::text::TextStyle;
use crate::theme::DARK_THEME;
use crate::ElementState;
use crate::widgets::frame::{Frame, FrameStyle, FrameStyleOverride};
use crate::widgets::text::Text;
use crate::{text, Color};
use crate::layout::{Axis, SizeValue};

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
    frame.set_direction(Axis::Horizontal);
    frame.set_initial_gap(SizeValue::Stretch);
    frame.set_final_gap(SizeValue::Stretch);
    frame.set_width(SizeValue::MaxContent);
    frame.set_min_width(SizeValue::Fixed(80.0));
    frame.add_child(Text::new(text!( style(text_style) "{label}" )));

    let frame_weak = RcElement::downgrade(frame.clone());
    frame.state_changed.watch(move |state| {
        if let Some(frame) = frame_weak.upgrade() {
            if state.is_hovered() {
                frame.set_background_color(Color::from_hex("474029"));
            } else if state.is_active() {
                frame.set_background_color(Color::from_hex("4c3e0a"));
            } else {
                frame.set_background_color(Color::from_hex("211e13"));
            }
            if state.is_focused() {
                frame.set_border_color(DARK_THEME.accent_color);
            } else {
                frame.set_border_color(Color::from_hex("4c3e0a"));
            }
        }
    });

    frame
}
