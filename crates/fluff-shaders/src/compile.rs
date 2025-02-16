use crate::session::create_session;
use crate::Error;
use slang::Downcast;
use std::path::Path;

/// Compiles a shader module from a file and an entry point name.
/// 
/// See the [slang documentation](https://github.com/shader-slang/slang/blob/master/docs/command-line-slangc-reference.md#-profile) for a list of available profiles.
pub fn compile_shader_module(path: &Path, search_paths: &[&Path], entry_point_name: &str) -> Result<Vec<u8>, Error> {
    let path = path.canonicalize()?;
    let session = create_session(search_paths);
    let module = session
        .load_module(path.to_str().unwrap())
        .map_err(|err| Error::CompileError(err.to_string()))?;
    let entry_point = module
        .find_entry_point_by_name(entry_point_name)
        .ok_or_else(|| Error::EntryPointNotFound(entry_point_name.to_string()))?;
    let program =
        session.create_composite_component_type(&[module.downcast().clone(), entry_point.downcast().clone()])?;
    let program = program.link()?;
    let code = program.entry_point_code(0, 0)?;
    Ok(code.as_slice().to_vec())
}
