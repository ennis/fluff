//! Shader compilation utilities.

use slang::Downcast;
use std::cell::OnceCell;
use std::error::Error;
use std::ffi::CString;
use std::fmt;
use std::path::Path;

/// The profile with which to compile the shaders.
///
/// See the [slang documentation](https://github.com/shader-slang/slang/blob/master/docs/command-line-slangc-reference.md#-profile) for a list of available profiles.
// Not sure if the profile matters in practice, it seems that slang will
// add the required SPIR-V extensions depending on what is used in the shader.
pub const SHADER_PROFILE: &str = "glsl_460";

fn get_slang_global_session() -> slang::GlobalSession {
    thread_local! {
        static SESSION: OnceCell<slang::GlobalSession> = OnceCell::new();
    }

    SESSION.with(|s| {
        s.get_or_init(|| slang::GlobalSession::new().expect("Failed to create Slang session"))
            .clone()
    })
}

pub(crate) fn create_session(
    profile_id: &str,
    search_paths: &[&Path],
    macro_definitions: &[(&str, &str)],
) -> slang::Session {
    let global_session = get_slang_global_session();

    let mut search_paths_cstr = vec![];
    for path in search_paths {
        search_paths_cstr.push(CString::new(path.to_str().unwrap()).unwrap());
    }
    let search_path_ptrs = search_paths_cstr.iter().map(|p| p.as_ptr()).collect::<Vec<_>>();

    let profile = global_session.find_profile(profile_id);
    let mut compiler_options = slang::CompilerOptions::default()
        .glsl_force_scalar_layout(true)
        .matrix_layout_column(true)
        .optimization(slang::OptimizationLevel::Default)
        .vulkan_use_entry_point_name(true)
        .profile(profile);

    for (k, v) in macro_definitions {
        compiler_options = compiler_options.macro_define(k, v);
    }

    let target_desc = slang::TargetDesc::default()
        .format(slang::CompileTarget::Spirv)
        .options(&compiler_options);
    let targets = [target_desc];

    let session_desc = slang::SessionDesc::default()
        .targets(&targets)
        .search_paths(&search_path_ptrs)
        .options(&compiler_options);

    let session = global_session
        .create_session(&session_desc)
        .expect("failed to create session");
    session
}

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

pub(crate) fn convert_spirv_u8_to_u32(bytes: &[u8]) -> Vec<u32> {
    assert!(bytes.len() % 4 == 0, "invalid SPIR-V code length");
    bytes
        .chunks_exact(4)
        .map(|chunk| {
            let bytes: [u8; 4] = chunk.try_into().unwrap();
            u32::from_ne_bytes(bytes)
        })
        .collect::<Vec<u32>>()
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
    entry_point_name: &str,
) -> Result<Vec<u32>, CompilationError> {
    let path = path.canonicalize()?;
    let session = create_session(SHADER_PROFILE, search_paths, macro_definitions);
    let module = session.load_module(path.to_str().unwrap())?;
    let entry_point = module
        .find_entry_point_by_name(entry_point_name)
        .ok_or_else(|| CompilationError::EntryPointNotFound(entry_point_name.to_string()))?;
    let program =
        session.create_composite_component_type(&[module.downcast().clone(), entry_point.downcast().clone()])?;
    let program = program.link()?;
    let code = program.entry_point_code(0, 0)?;
    Ok(convert_spirv_u8_to_u32(code.as_slice()))
}
