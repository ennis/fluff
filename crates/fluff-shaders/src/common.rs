use std::borrow::Cow;

/// The profile with which to compile the shaders.
///
/// See the [slang documentation](https://github.com/shader-slang/slang/blob/master/docs/command-line-slangc-reference.md#-profile) for a list of available profiles.
pub const SHADER_PROFILE: &str = "glsl_460";

/// Represents a shader type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Compute,
    Vertex,
    Fragment,
    Geometry,
    TessellationControl,
    TessellationEvaluation,
    Mesh,
    Task,
}

pub(crate) fn convert_slang_stage(stage: slang::Stage) -> Stage {
    match stage {
        slang::Stage::Compute => Stage::Compute,
        slang::Stage::Vertex => Stage::Vertex,
        slang::Stage::Fragment => Stage::Fragment,
        slang::Stage::Geometry => Stage::Geometry,
        slang::Stage::Domain => Stage::TessellationControl,
        slang::Stage::Hull => Stage::TessellationEvaluation,
        slang::Stage::Mesh => Stage::Mesh,
        slang::Stage::Amplification => Stage::Task,
        _ => panic!("unsupported shader stage: {:?}", stage),
    }
}

/// Represents an entry point for a shader.
pub struct EntryPoint<'a> {
    /// Shader stage.
    pub stage: Stage,
    /// Name of the entry point in SPIR-V code.
    pub name: Cow<'a, str>,
    /// Path to the source code for the shader.
    pub source_path: Option<Cow<'a, str>>,
    /// SPIR-V code for the entry point.
    pub code: Cow<'a, [u8]>,
    /// Size of the push constants in bytes.
    pub push_constants_size: u32,
    /// Size of the local workgroup in each dimension, if applicable to the shader type.
    ///
    /// This is valid for compute, task, and mesh shaders.
    pub workgroup_size: (u32, u32, u32),
}
