//! Shader compiler and binding generator.
//!
//! This crate serves two purposes:
//! - inside build scripts, it can be used to generate rust interface code for all shaders in a directory.
//! - at runtime, it can compile shader modules and generate reflection data.

use crate::parse::parse_file;
use anyhow::anyhow;
use heck::ToShoutySnakeCase;
use proc_macro2::TokenStream;
use quote::{format_ident, quote, TokenStreamExt};
use slang::{CompilerOptions, Downcast, GlobalSession, Module, Session, TargetDesc};
use std::error::Error;
use std::ffi::{CString, OsStr};
use std::path::{Path, PathBuf};
use std::{env, fs};
use tracing::{error, info};

mod gen;
mod lexer;
mod parse;
mod reflect;
mod utils;

/// Generates a GLSL include file containing definitions from the specified Rust file.
///
/// # Arguments
/// * `path` - Path to the Rust file containing the modules (annotated with `#[shader_bridge]`) to include in the generated GLSL file.
/// * `output_path` - Path to the output GLSL file.
///
/// # Example
///
/// Given the following module:
/// ```rust
/// /// 3D bezier control point.
/// #[repr(C)]
/// pub struct ControlPoint {
///     /// Position.
///     pub pos: [f32; 3],
///     /// RGB color.
///     pub color: [f32; 3],
/// }
/// ```
/// it will generate the following GLSL source:
/// ```glsl
/// struct ControlPoint {
///    vec3 pos;
///    vec3 color;
/// };
/// ```
///
#[deprecated(note = "use slang shaders instead")]
pub fn generate_shader_bridge(path: &str, output_path: &str) -> Result<(), Box<dyn Error>> {
    println!("cargo::rerun-if-changed={}", path);
    let contents = fs::read_to_string(path)?;
    let (bridgemod, errors) = parse_file(syn::parse_file(&contents)?);
    for err in errors.errors {
        println!(
            "cargo::warning={}:{}:{}:{}",
            path,
            err.span().start().line,
            err.span().start().column,
            err
        );
    }
    let mut output = Vec::new();
    gen::write_module(&bridgemod, &mut output);
    fs::write(output_path, String::from_utf8(output)?)?;
    Ok(())
}

/// Options for the `generate_slang_rust_bindings` function.
pub struct SlangRustBindingOptions<'a> {
    /// Slang source file containing additional declarations to translate (syntactically) to rust code.
    pub slang_declarations_file: Option<&'a str>,
    /// Where to write the generated rust code (relative to `OUT_DIR`).
    pub output: &'a str,
}

type CompilationErrors = Vec<(PathBuf, slang::Error)>;

fn load_modules_in_directory(
    session: &Session,
    directory: &Path,
    errors: &mut CompilationErrors,
) -> Result<Vec<Module>, anyhow::Error> {
    // load all modules in the search paths
    let mut modules = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let Some(ext) = path.extension() else { continue };
            if ext == OsStr::new("slang") {
                // re-run the build script if the slang file changes
                println!("cargo:rerun-if-changed={}", path.to_str().unwrap());
                let module_file_name = path.file_name().unwrap().to_str().unwrap();
                info!("loading module: {}", module_file_name);
                let module = match session.load_module(module_file_name) {
                    Ok(module) => module,
                    Err(err) => {
                        errors.push((path.clone(), err));
                        //let path = path.display();
                        //eprintln!();
                        //eprintln!("failed to load module `{path}`:");
                        //eprintln!("{}", err);
                        //eprintln!();
                        continue;
                    }
                };
                modules.push(module);
            }
        }
    }
    Ok(modules)
}

fn get_output_directory() -> PathBuf {
    // get output directory from build-time environment if we're compiling a crate,
    // or from runtime environment if we're running a build script.
    option_env!("OUT_DIR")
        .map(PathBuf::from)
        .or_else(|| env::var("OUT_DIR").ok().map(PathBuf::from))
        .expect("could not determine output directory")
}

/// Parses a `options.txt` file containing command line options for slang.
fn load_options_file(
    global_session: &GlobalSession,
    shaders_directory: &Path,
) -> Result<(CompilerOptions, TargetDesc<'static>), anyhow::Error> {
    let options_file = shaders_directory.join("options.txt");
    if !fs::exists(&options_file)? {
        eprintln!("error: options file not found: {}", options_file.display());
        return Ok((CompilerOptions::default(), TargetDesc::default()));
    }

    let mut compiler_options = CompilerOptions::default();
    let mut target_desc = TargetDesc::default();

    let contents = fs::read_to_string(options_file)?;
    let mut words = contents.split_whitespace();

    while let Some(option) = words.next() {
        match option {
            "-fvk-use-entrypoint-name" => {
                // TODO
            }
            "-fvk-use-scalar-layout" | "-force-glsl-scalar-layout" => {
                compiler_options = compiler_options.glsl_force_scalar_layout(true);
            }
            "-matrix-layout-row-major" => {
                compiler_options = compiler_options.matrix_layout_row(true);
            }
            "-matrix-layout-column-major" => {
                compiler_options = compiler_options.matrix_layout_column(true);
            }
            "-profile" => {
                if let Some(profile) = words.next() {
                    target_desc = target_desc.profile(global_session.find_profile(profile));
                } else {
                    eprintln!("error: -profile: missing profile id");
                    return Err(anyhow!("-profile: missing profile id"));
                }
            }
            "-O0" => {
                compiler_options = compiler_options.optimization(slang::OptimizationLevel::None);
            }
            "-O1" => {
                compiler_options = compiler_options.optimization(slang::OptimizationLevel::Default);
            }
            "-O2" => {
                compiler_options = compiler_options.optimization(slang::OptimizationLevel::High);
            }
            "-O3" => {
                compiler_options = compiler_options.optimization(slang::OptimizationLevel::Maximal);
            }
            other => {
                if other.starts_with("-D") {
                    let mut rest = other.split_at(2).1.trim();
                    if rest.is_empty() {
                        rest = words.next().unwrap_or("");
                    }
                    assert!(!rest.is_empty(), "invalid -D option");
                    let (key, value) = if let Some((key, value)) = rest.split_once('=') {
                        (key, value)
                    } else {
                        (rest, "")
                    };
                    compiler_options = compiler_options.macro_define(key, value);
                } else if other.starts_with("-I") {
                    // include search path
                    let rest = other.split_at(2).1.trim();
                    // TODO
                }
            }
        }
    }

    Ok((compiler_options, target_desc))
}

/// Returns the size of push constants used by the specified entry point.
fn push_constant_buffer_size(entry_point: &slang::reflection::EntryPoint) -> usize {
    // push constant are entry point function parameters with kind uniform
    let mut size = 0;
    for p in entry_point.parameters() {
        eprintln!("param category: {:?}", p.category());
        // There's a PushConstantBuffer category, but it doesn't seem to be used
        if p.category() == slang::ParameterCategory::Uniform {
            size += p.type_layout().size(slang::ParameterCategory::Uniform);
            eprintln!("param size: {}", size);
        }
    }
    size
}

/// Recompiles all entry points in the specified directory.
fn process_shaders(
    global_session: &GlobalSession,
    shaders_directory: &Path,
    output_directory: &Path,
    additional_macro_definitions: &[(&str, &str)],
    binding_options: Option<&SlangRustBindingOptions>,
) -> Result<(), anyhow::Error> {
    // sanity checks
    if shaders_directory.to_str().is_none() || output_directory.to_str().is_none() {
        panic!("invalid UTF-8 in path: this is unsupported");
    }

    let (mut compiler_options, mut target_desc) = load_options_file(global_session, shaders_directory).unwrap();
    target_desc = target_desc.format(slang::CompileTarget::Spirv);
    let search_paths = vec![CString::new(shaders_directory.to_str().unwrap()).unwrap()];
    let search_path_ptrs = search_paths.iter().map(|p| p.as_ptr()).collect::<Vec<_>>();
    let targets = [target_desc];

    for (key, value) in additional_macro_definitions {
        compiler_options = compiler_options.macro_define(key, value);
    }

    let session_desc = slang::SessionDesc::default()
        .targets(&targets)
        .search_paths(&search_path_ptrs)
        .options(&compiler_options);

    let session = global_session
        .create_session(&session_desc)
        .expect("failed to create session");

    let mut compilation_errors = CompilationErrors::new();
    let modules = load_modules_in_directory(&session, shaders_directory, &mut compilation_errors)?;
    let mut bindings = TokenStream::new();

    // Generate bindings if requested
    if let Some(_) = binding_options {
        let shaders_directory_str = shaders_directory.to_str().unwrap();

        let tokens = quote! {
            pub struct EntryPoint {
                pub path: Option<&'static str>,
                pub code: &'static [u8],
                pub push_constant_size: usize,
            }

            pub const SHADER_DIRECTORY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/", #shaders_directory_str);
        };

        bindings.append_all(tokens);

        // reflect compilation errors
        for (path, err) in compilation_errors {
            let path = path.display().to_string();
            let message = format!("failed to compile module `{path}`:\n{err}");

            bindings.append_all(quote! {
                compile_error!(#message);
            });
        }

        // verbatim decl file
        if let Some(slang_decl_file) = binding_options.unwrap().slang_declarations_file {
            let slang_decl_file_path = shaders_directory.join(slang_decl_file);
            let contents = fs::read_to_string(slang_decl_file_path)?;
            let tokens = lexer::translate(slang_decl_file, &contents);
            bindings.append_all(tokens);
        }
    }

    let mut ctx = reflect::Ctx::new();

    // Compile shaders
    {
        for module in modules.iter() {
            let mut components = Vec::new();
            components.push(module.downcast().clone());
            let entry_point_count = module.entry_point_count();
            for i in 0..entry_point_count {
                let entry_point = module.entry_point_by_index(i).unwrap();
                components.push(entry_point.downcast().clone());
            }
            let program = session.create_composite_component_type(&components).unwrap();
            let reflection = program.layout(0).expect("failed to get reflection");

            if binding_options.is_some() {
                // generate interface bindings
                ctx.generate_interface(reflection);
            }

            for i in 0..entry_point_count {
                let ep = module.entry_point_by_index(i).unwrap();
                let module_file_path = PathBuf::from(module.file_path());
                let module_file_stem = module_file_path
                    .file_stem()
                    .unwrap_or(module_file_path.file_name().unwrap())
                    .to_str()
                    .expect("invalid unicode file name");
                let entry_point_name = ep.function_reflection().name();

                let code = program.entry_point_code(i as i64, 0).unwrap();
                let output_file_name = format!("{module_file_stem}-{entry_point_name}.spv");
                let output_file = output_directory.join(&output_file_name);
                fs::write(&output_file, code.as_slice())?;

                if binding_options.is_some() {
                    let rust_entry_point_name = entry_point_name.to_shouty_snake_case();
                    let rust_entry_point_name = format_ident!("{rust_entry_point_name}");

                    let push_constant_buffer_size =
                        push_constant_buffer_size(&reflection.entry_point_by_index(i).unwrap());

                    //let output_file_str = output_file.to_str().unwrap();
                    bindings.append_all(quote! {
                        pub const #rust_entry_point_name: EntryPoint = EntryPoint {
                            path: Some(concat!(env!("OUT_DIR"), "/", #output_file_name)),
                            code: include_bytes!(concat!(env!("OUT_DIR"), "/", #output_file_name)),
                            push_constant_size: #push_constant_buffer_size
                        };
                    });
                }
            }
        }
    }

    // Write bindings to file
    if let Some(options) = binding_options {
        let path = output_directory.join(options.output);
        bindings.append_all(ctx.finish());
        fs::write(&path, bindings.to_string())?;
        utils::rustfmt_file(&path);
    }

    Ok(())
}

/// A list of macro definitions.
pub type MacroDefinitions = Vec<(String, String)>;

/// Generates rust bindings for data structures used in slang shader interfaces.
///
/// This will scan the specified directory for `.slang` files and generate rust code for the data
/// structures used in external shader interfaces (uniforms, push constants, etc.).
pub fn generate_slang_rust_bindings(
    shaders_directory: impl AsRef<Path>,
    additional_macro_definitions: &[(&str, &str)],
    options: &SlangRustBindingOptions,
) -> Result<(), anyhow::Error> {
    let global_session = slang::GlobalSession::new().unwrap();
    let output_directory = get_output_directory();
    process_shaders(
        &global_session,
        shaders_directory.as_ref(),
        &output_directory,
        additional_macro_definitions,
        Some(options),
    )?;
    Ok(())
}

pub fn recompile_shaders(
    shaders_directory: impl AsRef<Path>,
    additional_macro_definitions: &[(&str, &str)],
) -> Result<(), anyhow::Error> {
    let global_session = slang::GlobalSession::new().unwrap();
    let output_directory = get_output_directory();
    process_shaders(
        &global_session,
        shaders_directory.as_ref(),
        &output_directory,
        additional_macro_definitions,
        None,
    )?;
    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use testdir::testdir;

    #[test]
    fn test_generate_shader_bridge() {
        let source = r#"
        use shader_bridge_types::prelude::*;

        /// 3D bezier control point.
        #[repr(C)]
        pub struct ControlPoint {
            /// Position.
            pub pos: [f32; 3],
            /// RGB color.
            pub color: [f32; 3],
        }


        /// Represents a range of control points in the position buffer.
        #[derive(Copy, Clone)]
        #[repr(C)]
        pub struct CurveDesc {
            /// Width profile (polynomial coefficients).
            pub width_profile: Vec4,
            /// Opacity profile (polynomial coefficients).
            pub opacity_profile: Vec4,
            pub start: u32,
            /// Number of control points in the range.
            ///
            /// Should be 3N+1 for cubic Bézier curves.
            pub count: u32,
            /// parameter range
            pub param_range: Vec2,
        }

        const MAX_LINES_PER_TILE: usize = 16;

        #[derive(Copy, Clone)]
        #[repr(C)]
        pub struct TileLineData {
            pub coords: Vec4,
            pub param_range: Vec2,
            pub curve_index: u32,
        }

        #[derive(Copy, Clone)]
        #[repr(C)]
        pub struct TileData {
            pub lines: [TileLineData; MAX_LINES_PER_TILE],
        }


        #[derive(Copy, Clone)]
        #[repr(C)]
        pub struct BinCurvesParams {
            pub b_control_points: Buffer<ControlPoint>,
            pub b_curves: Buffer<CurveDesc>,
            pub b_tile_line_count: Buffer<u32>,
            pub b_tile_data: Buffer<TileData>,
            pub view_projection_matrix: Mat4,
            pub viewport_size: UVec2,
            pub stroke_width: f32,
            pub base_curve_index: i32,
            pub curve_count: i32,
            pub tiles_count_x: i32,
            pub tiles_count_y: i32,
            pub frame: i32,
        }

        pub const TILE_SIZE: i32 = 16;
        "#;

        let (bridgemod, errors) = parse_file(syn::parse_str(source).unwrap());
        for err in errors.errors {
            eprintln!("{}", err);
        }
        let mut output = Vec::new();
        gen::write_module(&bridgemod, &mut output);
        eprintln!("{}", String::from_utf8(output).unwrap());
    }

    #[test]
    fn compile_and_generate_bindings() {
        env_logger::init();
        let global_session = slang::GlobalSession::new().unwrap();
        let shaders_directory = Path::new("../fluff/slang");
        let output_directory = testdir!();
        process_shaders(
            &global_session,
            shaders_directory,
            &output_directory,
            &[],
            Some(&SlangRustBindingOptions { slang_declarations_file: Some("shared.slang"), output: "bindings.rs" }),
        )
        .unwrap();
    }
}
