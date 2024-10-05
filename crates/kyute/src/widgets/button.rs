use std::rc::Rc;

use kurbo::Vec2;
use smallvec::smallvec;
use taffy::{Dimension, LengthPercentage};
use taffy::prelude::max_content;

use crate::drawing::BoxShadow;
use crate::layout::flex::{CrossAxisAlignment, MainAxisAlignment};
use crate::style::{Style, StyleExt};
use crate::text::TextStyle;
use crate::theme::DARK_THEME;
use crate::widgets::frame::{Frame, FrameStyle, FrameStyleOverride, InteractState, TaffyStyle};
use crate::widgets::text::Text;
use crate::{text, Color};

fn button_style() -> FrameStyle {
    thread_local! {
        pub static BUTTON_STYLE: FrameStyle =

        FrameStyle {
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

        /*{
            let active = FrameStyle::new()
                .background_color(Color::from_hex("4c3e0a"))
                .box_shadows(vec![]);
            let focused = Style::new().border_color(DARK_THEME.accent_color);
            let hovered = Style::new().background_color(Color::from_hex("474029"));
            let s = Style::new()
                .background_color(Color::from_hex("211e13"))
                .border_radius(8.0)
                //.width(Sizing::MaxContent)
                //.height(Sizing::MaxContent)
                .min_width(200.0.into())
                .min_height(50.0.into())
                .padding_left(3.0.into())
                .padding_right(3.0.into())
                .padding_top(3.0.into())
                .padding_bottom(3.0.into())
                .border_color(Color::from_hex("4c3e0a"))
                .border_left(1.0.into())
                .border_right(1.0.into())
                .border_top(1.0.into())
                .border_bottom(1.0.into())
                .cross_axis_alignment(CrossAxisAlignment::Center)
                .main_axis_alignment(MainAxisAlignment::Center)
                .box_shadows(vec![
                    /*BoxShadow {
                        color: Color::from_rgb_u8(115, 115, 115),
                        offset: Vec2::new(0.0, 1.0),
                        blur: 0.0,
                        spread: 0.0,
                        inset: true,
                    },*/
                    BoxShadow {
                        color: Color::from_hex("4c3e0a"),
                        offset: Vec2::new(0.0, 1.0),
                        blur: 2.0,
                        spread: -1.0,
                        inset: false,
                    },
                ])
                .active(active)
                .hover(hovered)
                .focus(focused);
            s
        };*/
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

    frame.set::<TaffyStyle>(taffy::Style {
        display: taffy::Display::Grid,
        grid_template_columns: vec![max_content()],
        grid_template_rows: vec![max_content()],
        size: taffy::Size {
            width: Dimension::Auto,
            height: Dimension::Auto,
        },
        min_size: taffy::Size {
            width: Dimension::Length(200.0),
            height: Dimension::Length(50.0),
        },
        padding: taffy::Rect {
            left: LengthPercentage::Length(3.0),
            right: LengthPercentage::Length(3.0),
            top: LengthPercentage::Length(3.0),
            bottom: LengthPercentage::Length(3.0),
        },

        border: taffy::Rect {
            left: LengthPercentage::Length(1.0),
            right: LengthPercentage::Length(1.0),
            top: LengthPercentage::Length(1.0),
            bottom: LengthPercentage::Length(1.0),
        },

        justify_content: Some(taffy::JustifyContent::Center),
        align_content: Some(taffy::AlignContent::Center),
        ..Default::default()
    });

    frame
}
