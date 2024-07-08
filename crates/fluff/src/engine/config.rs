//! Render pipeline configuration file parser.
use graal::vk;
use serde::Deserialize;

/// Value or variable reference.
enum ValueOrRef<T> {
    Value(T),
    Ref(String),
}

struct Var {
    name: String,
    value: serde_json::Value,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Deserialize)]
enum SamplerMinFilter {
    Nearest,
    Linear,
    NearestMipmapNearest,
    LinearMipmapNearest,
    NearestMipmapLinear,
    LinearMipmapLinear,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Deserialize)]
enum SamplerMagFilter {
    Nearest,
    Linear,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Deserialize)]
enum SamplerAddressMode {
    ClampToEdge,
    ClampToBorder,
    Repeat,
    MirroredRepeat,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Deserialize)]
struct Sampler {
    min_filter: SamplerMinFilter,
    mag_filter: SamplerMagFilter,
    address_u: SamplerAddressMode,
    address_v: SamplerAddressMode,
    address_w: SamplerAddressMode,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
struct ImageFormat(vk::Format);


struct RenderTarget {
    format: ImageFormat,
    width: Option<ValueOrRef<u32>>,
    height: Option<ValueOrRef<u32>>,
    width_divisor: Option<ValueOrRef<u32>>,
    height_divisor: Option<ValueOrRef<u32>>,
}

struct Config {
    render_targets: Vec<RenderTarget>,
    samplers: Vec<Sampler>,
}