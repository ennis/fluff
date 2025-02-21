//! Shader compilation utilities.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use slang::Downcast;
use std::path::Path;

mod session;

pub(crate) use session::create_session;

/// The profile with which to compile the shaders.
///
/// See the [slang documentation](https://github.com/shader-slang/slang/blob/master/docs/command-line-slangc-reference.md#-profile) for a list of available profiles.
// Not sure if the profile matters in practice, it seems that slang will
// add the required SPIR-V extensions depending on what is used in the shader.
pub const SHADER_PROFILE: &str = "glsl_460";

/// Compilation error.
#[derive(Debug)]
pub enum CompilationError {
    //#[error("compilation errors: {0}")]
    //CompileError(String),
    IoError(std::io::Error),
    EntryPointNotFound(String),
    SlangError(slang::Error),
}

impl fmt::Display for CompilationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            //CompilationError::CompileError(err) => write!(f, "compilation errors: {}", err),
            CompilationError::IoError(err) => write!(f, "I/O error: {:?}", err),
            CompilationError::EntryPointNotFound(name) => write!(f, "entry point not found: {}", name),
            CompilationError::SlangError(err) => write!(f, "compilation errors: {}", err),
        }
    }
}

impl Error for CompilationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            CompilationError::IoError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<slang::Error> for CompilationError {
    fn from(err: slang::Error) -> Self {
        CompilationError::SlangError(err)
    }
}

impl From<std::io::Error> for CompilationError {
    fn from(err: std::io::Error) -> Self {
        CompilationError::IoError(err)
    }
}


/// Compiles a shader module to SPIR-V.
///
/// # Arguments
/// * `path` - Path to the shader module.
/// * `profile` - Profile with which to compile the shaders.
/// * `search_paths` - Paths to search for included files.
/// * `entry_point_name` - Name of the entry point in the shader module.
///
pub fn compile_shader_module(
    path: &Path,
    search_paths: &[&Path],
    macro_definitions: &[(&str, &str)],
    entry_point_name: &str) -> Result<Vec<u8>, CompilationError>
{
    let path = path.canonicalize()?;
    let session = create_session(SHADER_PROFILE, search_paths, macro_definitions);
    let module = session
        .load_module(path.to_str().unwrap())?;
    let entry_point = module
        .find_entry_point_by_name(entry_point_name)
        .ok_or_else(|| CompilationError::EntryPointNotFound(entry_point_name.to_string()))?;
    let program =
        session.create_composite_component_type(&[module.downcast().clone(), entry_point.downcast().clone()])?;
    let program = program.link()?;
    let code = program.entry_point_code(0, 0)?;
    Ok(code.as_slice().to_vec())
}
