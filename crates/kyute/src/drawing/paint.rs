//! Description of paints.
use kurbo::{Rect, Vec2};
use skia_safe as sk;
use skia_safe::gradient_shader::GradientShaderColors;
use tracing::warn;

use crate::drawing::{Image, LinearGradient, ToSkia};
use crate::Color;

/// Image repeat mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum RepeatMode {
    Repeat,
    NoRepeat,
}

/// Data passed to uniforms.
#[derive(Clone, Debug)]
pub struct UniformData(pub sk::Data);

/// Macro to create the uniform data for a shader.
#[macro_export]
macro_rules! make_uniform_data {
    ( [$effect:ident] $($name:ident : $typ:ty = $value:expr),*) => {
        unsafe {
            let total_size = $effect.uniform_size();
            let mut data: Vec<u8> = Vec::with_capacity(total_size);
            let ptr = data.as_mut_ptr();

            $(
            {
                let (u_offset, u_size) = $effect
                    .uniforms()
                    .iter()
                    .find_map(|u| {
                        if u.name() == std::stringify!($name) {
                            Some((u.offset(), u.size_in_bytes()))
                        } else {
                            None
                        }
                    })
                    .expect("could not find uniform");

                let v : $typ = $value;
                assert_eq!(std::mem::size_of::<$typ>(), u_size);
                std::ptr::write(ptr.add(u_offset).cast::<$typ>(), v);
            }
            )*

            data.set_len(total_size);
            $crate::drawing::UniformData(skia_safe::Data::new_copy(&data))
        }
    };
}

#[macro_export]
macro_rules! shader {
    ($source:literal) => {{
        thread_local! {
            static SHADER: std::cell::OnceCell<$crate::skia::RuntimeEffect> = OnceCell::new();
        }
        SHADER.with(|cell| {
            cell.get_or_init(|| {
                $crate::skia::RuntimeEffect::make_for_shader($source, None).expect("failed to compile shader")
            })
            .clone()
        })

        /*static SHADER: std::sync::OnceLock<$crate::ThreadBound<$crate::skia::RuntimeEffect>> =
            std::sync::OnceLock::new();
        SHADER
            .get_or_init(|| {
                $crate::skia::RuntimeEffect::make_for_shader($source, None).expect("failed to compile shader")
            })
            .get_ref()
            .expect("shader accessed from another thread")*/
    }};
}

/// Creates a `Paint` object from the specified shader and uniforms.
#[macro_export]
macro_rules! shader_paint {
    ($source:literal, $($name:ident : $typ:ty = $value:expr),*) => {
        {
            let shader = $crate::shader!($source).clone();
            let uniforms = $crate::make_uniform_data!([shader] $($name : $typ = $value),*);
            $crate::drawing::Paint::Shader { effect: shader, uniforms }
        }
    };
}

/*
fn compare_runtime_effects(left: &sk::RuntimeEffect, right: &sk::RuntimeEffect) -> bool {
    // FIXME: skia_safe doesn't let us access the native pointer for some reason,
    // so force our way though
    //left.native() as *const _ == right.native() as *const _
    unsafe {
        let ptr_a: *const c_void = mem::transmute_copy(left);
        let ptr_b: *const c_void = mem::transmute_copy(right);
        ptr_a == ptr_b
    }
}
*/

/// Paint.
#[derive(Clone, Debug)]
//#[serde(tag = "type")]
pub enum Paint {
    //#[serde(rename = "color")]
    Color(Color),
    //#[serde(rename = "linear-gradient")]
    LinearGradient(LinearGradient),
    //#[serde(rename = "image")]
    Image {
        // FIXME: can't deserialize here
        image: Image,
        repeat_x: RepeatMode,
        repeat_y: RepeatMode,
    },
    // TODO: shader effects
    Shader {
        // GOD FCKING DAMMIT MAKE THIS THREAD-SAFE ALREADY
        effect: sk::RuntimeEffect,
        uniforms: UniformData,
    },
}

impl PartialEq for Paint {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Paint::Color(a), Paint::Color(b)) => a == b,
            (Paint::LinearGradient(a), Paint::LinearGradient(b)) => a == b,
            (Paint::Image { .. }, Paint::Image { .. }) => {
                // TODO
                false
            }
            (Paint::Shader { .. }, Paint::Shader { .. }) => {
                // TODO
                false
            }
            _ => false,
        }
    }
}

impl Default for Paint {
    fn default() -> Self {
        Paint::Color(Color::new(0.0, 0.0, 0.0, 0.0))
    }
}

impl Paint {
    pub fn is_transparent(&self) -> bool {
        if let Paint::Color(color) = self {
            color.alpha() == 0.0
        } else {
            false
        }
    }

    /// Converts this object to a skia `SkPaint`.
    pub fn to_sk_paint(&self, bounds: Rect, style: skia_safe::PaintStyle) -> sk::Paint {
        let mut paint = match self {
            Paint::Color(color) => {
                let mut paint = sk::Paint::new(color.to_skia(), None);
                paint.set_anti_alias(true);
                paint
            }
            Paint::LinearGradient(linear_gradient) => linear_gradient.to_skia_paint(bounds),
            Paint::Image {
                image,
                repeat_x,
                repeat_y,
            } => {
                let tile_x = match *repeat_x {
                    RepeatMode::Repeat => sk::TileMode::Repeat,
                    RepeatMode::NoRepeat => sk::TileMode::Decal,
                };
                let tile_y = match *repeat_y {
                    RepeatMode::Repeat => sk::TileMode::Repeat,
                    RepeatMode::NoRepeat => sk::TileMode::Decal,
                };
                let sampling_options = sk::SamplingOptions::new(sk::FilterMode::Linear, sk::MipmapMode::None);
                let image_shader = image
                    .to_skia()
                    .to_shader((tile_x, tile_y), sampling_options, None)
                    .unwrap();
                let mut paint = sk::Paint::default();
                paint.set_shader(image_shader);
                paint
            }
            Paint::Shader { effect, uniforms } => {
                let shader = effect
                    .make_shader(&uniforms.0, &[], None)
                    .expect("failed to create shader");
                let mut paint = sk::Paint::default();
                paint.set_shader(shader);
                paint
            }
        };
        paint.set_style(style);
        paint
    }
}

impl From<Color> for Paint {
    fn from(color: Color) -> Self {
        Paint::Color(color)
    }
}

impl From<LinearGradient> for Paint {
    fn from(g: LinearGradient) -> Self {
        Paint::LinearGradient(g)
    }
}

/*fn deserialize_angle<'de, D>(d: D) -> Result<Angle, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = Angle;
        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("floating-point value")
        }
        fn visit_f64<E>(self, value: f64) -> Result<Angle, E>
        where
            E: serde::de::Error,
        {
            Ok(Angle::radians(value))
        }
    }

    d.deserialize_f32(Visitor)
}*/

/*/// From CSS value.
impl TryFrom<&str> for Paint {
    type Error = ();
    fn try_from(css: &str) -> Result<Self, ()> {
        Paint::parse(css).map_err(|_| ())
    }
}*/
