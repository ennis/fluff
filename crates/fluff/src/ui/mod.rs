mod highlight;

use egui::{
    Align, Align2, Area, Color32, Direction, FontId, Frame, InnerResponse, Key, Layout, Order, Response, RichText, TextEdit, TextFormat,
    TextStyle, Ui,
};
use egui_dnd::dnd;
use egui_extras::Column;
use graal::{vk, Format};
use highlight::highlight;
use std::{fmt::Debug, hash::Hash};

/*
fn enum_combo<T: Debug + PartialEq + Copy>(ui: &Ui, label: &str, options: &[T], current: &mut T) {
    let mut selected = options.iter().position(|v| v == current).unwrap_or(0);
    let preview = format!("{:?}", current);
    if let Some(combo) = ui.begin_combo(label, preview) {
        for (i, v) in options.iter().enumerate() {
            let name = format!("{:?}", v);
            let clicked = ui.selectable_config(&name).selected(selected == i).build();
            if selected == i {
                ui.set_item_default_focus();
            }
            if clicked {
                selected = i;
            }
        }
        combo.end();
    }
    *current = options[selected];
}*/

fn describe_blend_factor(f: BlendFactor) -> &'static str {
    match f {
        BlendFactor::Zero => "0",
        BlendFactor::One => "1",
        BlendFactor::SrcColor => "C_src",
        BlendFactor::OneMinusSrcColor => "1 - C_src",
        BlendFactor::DstColor => "C_dst",
        BlendFactor::OneMinusDstColor => "1 - C_dst",
        BlendFactor::SrcAlpha => "α_src",
        BlendFactor::OneMinusSrcAlpha => "1 - α_src",
        BlendFactor::DstAlpha => "α_dst",
        BlendFactor::OneMinusDstAlpha => "1 - α_dst",
        BlendFactor::ConstantColor => "C_const",
        BlendFactor::OneMinusConstantColor => "1 - C_const",
        BlendFactor::ConstantAlpha => "α_const",
        BlendFactor::OneMinusConstantAlpha => "1 - α_const",
        BlendFactor::SrcAlphaSaturate => "min(α_src, 1 - α_dst)",
    }
}

fn describe_format(format: Format) -> (&'static str, Color32) {
    let color_formats_color = Color32::from_rgb(0x00, 0x00, 0xFF);
    let depth_formats_color = Color32::from_rgb(0xFF, 0x00, 0x00);

    match format {
        Format::UNDEFINED => ("UNDEFINED", Color32::WHITE),
        Format::R8G8B8A8_UNORM => ("R8G8B8A8_UNORM", color_formats_color),
        Format::B8G8R8A8_UNORM => ("B8G8R8A8_UNORM", color_formats_color),
        Format::R8G8B8A8_SRGB => ("R8G8B8A8_SRGB", color_formats_color),
        Format::B8G8R8A8_SRGB => ("B8G8R8A8_SRGB", color_formats_color),
        Format::R16G16B16A16_SFLOAT => ("R16G16B16A16_SFLOAT", color_formats_color),
        Format::R32G32B32A32_SFLOAT => ("R32G32B32A32_SFLOAT", color_formats_color),
        Format::D32_SFLOAT => ("D32_SFLOAT", depth_formats_color),
        Format::D24_UNORM_S8_UINT => ("D24_UNORM_S8_UINT", depth_formats_color),
        _ => ("Unknown", Color32::RED),
    }
}

fn vk_format_combo(ui: &mut Ui, id: impl Hash, selected: &mut Format) {
    let (preview, _color) = describe_format(*selected);
    egui::ComboBox::from_id_source(id)
        .selected_text(preview)
        .width(250.0)
        .show_ui(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::new(0.0, 0.0);
            for format in [
                Format::UNDEFINED,
                Format::R8G8B8A8_UNORM,
                Format::B8G8R8A8_UNORM,
                Format::R8G8B8A8_SRGB,
                Format::B8G8R8A8_SRGB,
                Format::R16G16B16A16_SFLOAT,
                Format::R32G32B32A32_SFLOAT,
                Format::D32_SFLOAT,
                Format::D24_UNORM_S8_UINT,
            ] {
                let (description, mut color) = describe_format(format);
                color = color.gamma_multiply(0.2);

                let width = ui.available_width();
                let height = 20.0;
                let (rect, resp) = ui.allocate_exact_size(egui::Vec2::new(width, height), egui::Sense::click());

                /*if resp.hovered() {
                    color = color.gamma_multiply(1.5);
                }*/
                ui.painter().rect_filled(rect, 0.0, color);
                {
                    let mut ui = ui.child_ui(rect, Layout::top_down_justified(Align::Min));
                    ui.selectable_value(selected, format, description);
                }
            }
        });
}

fn blend_factor_combo(ui: &mut Ui, id: &str, f: &mut BlendFactor) {
    let preview = describe_blend_factor(*f);
    egui::ComboBox::from_id_source(id)
        .selected_text(preview)
        .width(150.)
        .show_ui(ui, |ui| {
            for factor in [
                BlendFactor::Zero,
                BlendFactor::One,
                BlendFactor::SrcColor,
                BlendFactor::OneMinusSrcColor,
                BlendFactor::DstColor,
                BlendFactor::OneMinusDstColor,
                BlendFactor::SrcAlpha,
                BlendFactor::OneMinusSrcAlpha,
                BlendFactor::DstAlpha,
                BlendFactor::OneMinusDstAlpha,
                BlendFactor::ConstantColor,
                BlendFactor::OneMinusConstantColor,
                BlendFactor::ConstantAlpha,
                BlendFactor::OneMinusConstantAlpha,
                BlendFactor::SrcAlphaSaturate,
            ] {
                ui.selectable_value(f, factor, describe_blend_factor(factor));
            }
        });
}

// see https://github.com/rerun-io/rerun/blob/main/crates/re_ui/src/lib.rs#L599
fn generic_list_header<R>(ui: &mut egui::Ui, label: &str, right_buttons: impl Fn(&mut egui::Ui) -> R) {
    ui.allocate_ui_with_layout(
        egui::Vec2::new(ui.available_width(), 20.0),
        egui::Layout::left_to_right(Align::Center),
        |ui| {
            let mut rect = ui.available_rect_before_wrap();
            let hline_stroke = ui.style().visuals.widgets.noninteractive.bg_stroke;
            rect.extend_with_x(ui.clip_rect().right());
            rect.extend_with_x(ui.clip_rect().left());
            ui.painter().hline(rect.x_range(), rect.top(), hline_stroke);
            ui.painter().hline(rect.x_range(), rect.bottom(), hline_stroke);

            ui.strong(label);
            ui.allocate_ui_with_layout(ui.available_size(), egui::Layout::right_to_left(egui::Align::Center), right_buttons)
                .inner
        },
    );
}

pub(crate) fn style_to_text_format(style: &egui::Style) -> TextFormat {
    let mut text_format = TextFormat::default();
    text_format.color = style.visuals.text_color();
    text_format.font_id = style.override_text_style.clone().unwrap_or(TextStyle::Body).resolve(style);
    text_format
}

macro_rules! rich_text {

    ( @property($tf:ident) rgb ($r:expr, $g:expr, $b:expr) ) => {
        $tf.color = egui::Color32::from_rgb($r, $g, $b);
    };

    ( @parse($job:ident,$tf:ident) ) => {};

    ( @parse($job:ident,$tf:ident) $ident:ident $($rest:tt)* ) => {
        $job.append(&ident, 0.0, $tf.clone());
        rich_text!(@parse($job,$tf) $($rest)*);
    };

    ( @parse($job:ident,$tf:ident) $string:literal $($rest:tt)* ) => {
        match format_args!($string) {
            args if args.as_str().is_some() => $job.append(args.as_str().unwrap(), 0.0, $tf.clone()),
            args => $job.append(&args.to_string(), 0.0, $tf.clone()),
        }
        rich_text!(@parse($job,$tf) $($rest)*);
    };

    ( @parse($job:ident,$tf:ident) $(@$property:ident $(( $($args:expr),* ))? )* { $($inner:tt)* } $($rest:tt)* ) => {
        {
            let mut $tf = $tf.clone();
            $(rich_text!(@property($tf) $property $(( $($args),* ))?);)*
            rich_text!(@parse($job,$tf) $($inner)*);
        }
        rich_text!(@parse($job,$tf) $($rest)*);
    };

    ( $style:expr; $($rest:tt)* ) => {
        {
            let mut job = egui::text::LayoutJob::default();
            let tf = crate::ui::style_to_text_format($style);
            rich_text!(@parse(job, tf) $($rest)*);
            job
        }
    };
}

use crate::ui::highlight::CodeTheme;
pub(crate) use rich_text;

fn output_buffer_properties_popup(ui: &mut Ui, id: impl Hash) {}

fn output_buffer_properties(id: impl Hash, ui: &mut Ui) {
    #[derive(Copy, Clone, Default)]
    struct State {
        open: bool,
    }

    let id = ui.make_persistent_id(id);
    let mut state = ui.data_mut(|data| data.get_temp::<State>(id)).unwrap_or_default();

    let button_response = ui.button("Properties...");

    if button_response.clicked() {
        state.open = true;
        ui.data_mut(|data| data.insert_temp(id, state));
    }

    if state.open {
        let InnerResponse {
            inner,
            response: area_response,
        } = Area::new(id)
            .order(Order::Foreground)
            .fixed_pos(button_response.rect.center_bottom())
            .show(ui.ctx(), |ui| {
                let frame = Frame::popup(ui.style());
                frame
                    .show(ui, |ui| {
                        let mut size_expr = "len($input_buffer)".to_string();
                        let label = ui.label("Size (number of elements): ");
                        ui.text_edit_singleline(&mut size_expr).labelled_by(label.id);
                    })
                    .inner
            });

        if !button_response.clicked() && (ui.input(|i| i.key_pressed(Key::Escape)) || area_response.clicked_elsewhere()) {
            state.open = false;
            ui.data_mut(|data| data.insert_temp(id, state));
        }
    }
}

fn color_icon(icon: &str, color: Color32) -> RichText {
    RichText::new(icon).size(20.0).color(color)
}

fn icon_button(ui: &mut Ui, icon: &str, color: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::new(20.0, 20.0), egui::Sense::click());
    if response.hovered() {
        let color = ui.style().visuals.selection.bg_fill;
        ui.painter().rect_filled(rect, 0.0, color);
    }
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, icon, FontId::proportional(16.0), color);
    response
}

fn list_row(ui: &mut Ui, label: &str, value: &str) {}

struct BufferResource {
    name: String,
    usage: vk::BufferUsageFlags,
    scale_w: Option<f32>,
    scale_h: Option<f32>,
    size: Option<usize>,
}

struct ImageResource {
    name: String,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
    scale_w: Option<f32>,
    scale_h: Option<f32>,
    size: Option<(u32, u32)>,
}

enum Resource {
    Buffer(BufferResource),
    Image(ImageResource),
}

/*
fn resource_badge(ui: &mut Ui, r: &Resource) {
    match r {
        Resource::Buffer(_) => {
            //ui.allo
            egui::widgets::Label::new("Buffer")
                .text_color(egui::Color32::from_rgb(0, 0, 255))
                .show();
        }
        Resource::Image(_) => {
            egui::widgets::Label::new("Image")
                .text_color(egui::Color32::from_rgb(255, 0, 0))
                .show();
        }
    }
}*/

pub fn resources_window(ctx: &egui::Context) {
    let example_resources = vec![
        Resource::Buffer(BufferResource {
            name: "Vertices".to_string(),
            usage: vk::BufferUsageFlags::VERTEX_BUFFER,
            scale_w: Some(1.0),
            scale_h: Some(1.0),
            size: Some(1024),
        }),
        Resource::Buffer(BufferResource {
            name: "Indices".to_string(),
            usage: vk::BufferUsageFlags::INDEX_BUFFER,
            scale_w: Some(1.0),
            scale_h: Some(1.0),
            size: Some(1024),
        }),
        Resource::Image(ImageResource {
            name: "Color".to_string(),
            format: vk::Format::R8G8B8A8_UNORM,
            usage: vk::ImageUsageFlags::COLOR_ATTACHMENT,
            scale_w: Some(1.0),
            scale_h: Some(1.0),
            size: Some((1024, 1024)),
        }),
        Resource::Image(ImageResource {
            name: "Depth".to_string(),
            format: vk::Format::D32_SFLOAT,
            usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
            scale_w: Some(1.0),
            scale_h: Some(1.0),
            size: Some((1024, 1024)),
        }),
    ];

    egui::Window::new("Resources").show(ctx, |ui| {
        egui_extras::TableBuilder::new(ui)
            .columns(Column::initial(100.0), 6)
            .striped(true)
            .resizable(true)
            .header(30., |mut row| {
                row.col(|ui| {
                    ui.label("Name");
                });
                row.col(|ui| {
                    ui.label("Type");
                });
                row.col(|ui| {
                    ui.label("Usage");
                });
                row.col(|ui| {
                    ui.label("Scale W");
                });
                row.col(|ui| {
                    ui.label("Scale H");
                });
                row.col(|ui| {
                    ui.label("Size");
                });
            })
            .body(|mut body| {
                for r in example_resources {
                    match r {
                        Resource::Buffer(b) => {
                            body.row(15., |mut row| {
                                row.col(|ui| {
                                    ui.label(b.name);
                                });
                                row.col(|ui| {
                                    ui.label("Buffer");
                                });
                                row.col(|ui| {
                                    ui.label(format!("{:?}", b.usage));
                                });
                                row.col(|ui| {
                                    if let Some(sw) = b.scale_w {
                                        ui.label(format!("{}", sw));
                                    }
                                });
                                row.col(|ui| {
                                    if let Some(sh) = b.scale_h {
                                        ui.label(format!("{}", sh));
                                    }
                                });
                                row.col(|ui| {
                                    if let Some(b) = b.size {
                                        ui.label(format!("{:?}", b));
                                    }
                                });
                            });
                        }
                        Resource::Image(i) => {
                            body.row(15., |mut row| {
                                row.col(|ui| {
                                    ui.label(i.name);
                                });
                                row.col(|ui| {
                                    ui.label("Image");
                                });
                                row.col(|ui| {
                                    ui.label(format!("{:?}", i.usage));
                                });
                                row.col(|ui| {
                                    if let Some(sw) = i.scale_w {
                                        ui.label(format!("{:?}", sw));
                                    }
                                });
                                row.col(|ui| {
                                    if let Some(sh) = i.scale_h {
                                        ui.label(format!("{:?}", sh));
                                    }
                                });
                                row.col(|ui| {
                                    if let Some((w, h)) = i.size {
                                        ui.label(format!("{}×{}", w, h));
                                    }
                                });
                            });
                        }
                    }
                }
            })
    });
}

pub fn test_ui(ui: &mut egui::Ui) {
    let mut blend_src_rgb = BlendFactor::SrcAlpha;
    let mut blend_dst_rgb = BlendFactor::OneMinusSrcAlpha;
    let mut blend_src_alpha = BlendFactor::One;
    let mut blend_dst_alpha = BlendFactor::Zero;

    icon_button(ui, egui_phosphor::fill::PLUS, Color32::WHITE);

    egui::CollapsingHeader::new("Blending").show(ui, |ui| {
        ui.label("Orgb = srgb * Srgb + drgb * Drgb");
        ui.label("Oa = sa * Sa + da * Da");

        egui::Grid::new("blending").num_columns(4).show(ui, |ui| {
            ui.label("srgb:");
            ui.label("drgb:");
            ui.end_row();
            blend_factor_combo(ui, "srgb", &mut blend_src_rgb);
            blend_factor_combo(ui, "drgb", &mut blend_dst_rgb);
            ui.end_row();
            ui.label("sa:");
            ui.label("da:");
            ui.end_row();
            blend_factor_combo(ui, "sa", &mut blend_src_alpha);
            blend_factor_combo(ui, "da", &mut blend_dst_alpha);
            ui.end_row();
        });

        generic_list_header(ui, "Attachments", |ui| {
            if icon_button(ui, egui_phosphor::fill::PLUS, Color32::WHITE).on_hover_text("Add new attachment").clicked() {
                eprintln!("TODO: add new attachment")
            }
            if icon_button(ui, egui_phosphor::fill::TRASH, Color32::WHITE).on_hover_text("Remove selected attachment").clicked() {
                eprintln!("TODO: remove selected attachment")
            }
        });

        let mut current_attachment = 1;
        dnd(ui, "attachments").show([1, 2, 3].into_iter(), |ui, val, handle, state| {
            handle.show_drag_cursor_on_hover(false).ui(ui, |ui| {
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    ui.selectable_value(&mut current_attachment, val, format!("Attachment {}", val));
                    vk_format_combo(ui, ("format", val), &mut Format::UNDEFINED);
                });
            });
        });

        output_buffer_properties("output_buffer_properties", ui);


        let srgb = describe_blend_factor(blend_src_rgb);
        let drgb = describe_blend_factor(blend_dst_rgb);
        let sa = describe_blend_factor(blend_src_alpha);
        let da = describe_blend_factor(blend_dst_alpha);

        ui.label(rich_text!(ui.style(); "Orgb = " @rgb(255,0,0) { "({srgb})" }  " × Srgb" @rgb(255,255,0) { " + " } @rgb(255,0,0) { "({drgb})" } " × Drgb" ));

        let mut glsl_source = r#"
//#include "common.glsl"

#extension GL_EXT_mesh_shader : require
#extension GL_EXT_scalar_block_layout : require
#extension GL_ARB_fragment_shader_interlock : require

//////////////////////////////////////////////////////////

// A range of control points in the controlPoints buffer that describes a single curve.
struct CurveDescriptor {
    vec4 widthProfile; // polynomial coefficients
    vec4 opacityProfile; // polynomial coefficients
    uint start;
    uint size;
    vec2 paramRange;
};

// Per-fixel fragment data before sorting and blending
struct FragmentData {
    vec4 color;
    float depth;
};

//////////////////////////////////////////////////////////

struct ControlPoint {
    vec3 pos;
    vec3 color;
};

buffer PositionBuffer {
    ControlPoint[] controlPoints;
};

buffer CurveBuffer {
    CurveDescriptor[] curves;
};

// Push constants
uniform PushConstants {
    mat4 viewProjectionMatrix;
    uvec2 viewportSize;
    float strokeWidth;
    int baseCurveIndex;
    int curveCount;
    int tilesCountX;
    int tilesCountY;
    int frame;
};
}"#.to_string();

        let mut theme = CodeTheme::dark();
        theme.ui(ui);

        let mut layouter = |ui: &Ui, str: &str, wrap_width: f32| {
            let mut layout_job =
                highlight(ui.ctx(), &theme, str, "glsl");
            layout_job.wrap.max_width = wrap_width;
            ui.fonts(|f| f.layout_job(layout_job))
        };

        egui::ScrollArea::vertical().show(ui, |ui| {
            let code_editor = TextEdit::multiline(&mut glsl_source)
                .font(egui::TextStyle::Monospace) // for cursor height
                .code_editor()
                .desired_rows(20)
                .desired_width(f32::INFINITY).layouter(&mut layouter);
            ui.add(code_editor);
        });
    });
}
