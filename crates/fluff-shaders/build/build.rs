use crate::common::convert_slang_stage;
use crate::session::create_session;
use heck::ToShoutySnakeCase;
use proc_macro2::TokenStream;
use quote::{TokenStreamExt, format_ident, quote};
use slang::Downcast;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs, io};

mod bindgen;
#[path = "../src/common.rs"]
mod common;
#[path = "../src/session.rs"]
mod session;

/// Gets the rustfmt path to rustfmt the generated bindings.
fn rustfmt_path<'a>() -> PathBuf {
    if let Ok(rustfmt) = env::var("RUSTFMT") {
        rustfmt.into()
    } else {
        // just assume that it is in path
        "rustfmt".into()
    }
}

pub fn rustfmt_file(path: &Path) {
    let rustfmt = rustfmt_path();
    let mut cmd = Command::new(rustfmt);

    cmd.arg(path.as_os_str());
    match cmd.spawn() {
        Ok(mut child) => {
            if let Ok(exit_status) = child.wait() {
                if !exit_status.success() {
                    eprintln!("rustfmt failed (exit status = {exit_status})")
                }
            }
        }
        Err(err) => {
            eprintln!("failed to run rustfmt: {err}");
        }
    }
}

fn load_modules_in_directory(
    session: &slang::Session,
    shaders_directory: &Path,
) -> Result<Vec<slang::Module>, io::Error> {
    // load all modules in the search paths
    let mut modules = Vec::new();
    for entry in fs::read_dir(shaders_directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let Some(ext) = path.extension() else { continue };
            if ext == OsStr::new("slang") {
                let path_str = path.to_str().unwrap();
                // re-run the build script if the slang file changes
                println!("cargo:rerun-if-changed={path_str}");
                // load the module
                match session.load_module(path_str) {
                    Ok(module) => {
                        modules.push(module);
                    }
                    Err(err) => {
                        // output compilation errors
                        for line in err.to_string().lines() {
                            println!("cargo:warning={line}");
                        }
                        continue;
                    }
                };
            }
        }
    }
    Ok(modules)
}

/// Returns the size of push constants used by the specified entry point.
fn push_constant_buffer_size(entry_point: &slang::reflection::EntryPoint) -> usize {
    // push constant are entry point function parameters with kind uniform
    let mut size = 0;
    for p in entry_point.parameters() {
        // There's a PushConstantBuffer category, but it doesn't seem to be used
        if p.category() == slang::ParameterCategory::Uniform {
            size += p.type_layout().size(slang::ParameterCategory::Uniform);
        }
    }
    size
}

/// Compiles all shaders in the given directory and embeds the SPIR-V code in a rust module.
fn compile_and_embed_shaders(
    shaders_directory: &Path,
    include_search_paths: &[&Path],
    output_directory: &Path,
    bindings_file_name: &str,
) -> Result<(), io::Error> {
    let session = create_session(include_search_paths);
    let modules = load_modules_in_directory(&session, shaders_directory).unwrap();

    // now compile all entry points, and generate bindings
    let mut bindings = TokenStream::new();

    bindings.append_all(quote! {
        use std::borrow::Cow;
    });

    let mut ctx = bindgen::Ctx::new();

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

            ctx.generate_interface(&reflection);

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
                fs::write(&output_directory.join(&output_file_name), code.as_slice())?;

                let rust_entry_point_name = format_ident!("{}", entry_point_name.to_shouty_snake_case());

                let refl_ep = reflection.entry_point_by_index(i).unwrap();
                let push_constant_buffer_size = push_constant_buffer_size(&refl_ep);
                let stage = convert_slang_stage(refl_ep.stage());
                let stage = format_ident!("{stage:?}");
                let [workgroup_size_x, workgroup_size_y, workgroup_size_z] = refl_ep.compute_thread_group_size();

                //let output_file_str = output_file.to_str().unwrap();
                bindings.append_all(quote! {
                    pub const #rust_entry_point_name: EntryPoint<'static> = EntryPoint {
                        stage: crate::Stage::#stage,
                        name: Cow::Borrowed(#entry_point_name),
                        source_path: Some(Cow::Borrowed(concat!(env!("OUT_DIR"), "/", #output_file_name))),
                        code: Cow::Borrowed(include_bytes!(concat!(env!("OUT_DIR"), "/", #output_file_name))),
                        push_constants_size: #push_constant_buffer_size,
                        workgroup_size: (#workgroup_size_x, #workgroup_size_y, #workgroup_size_z),
                    };
                });
            }
        }
    }

    // Write bindings to file
    {
        let bindings_file_path = output_directory.join(bindings_file_name);
        bindings.append_all(ctx.finish());
        fs::write(&bindings_file_path, bindings.to_string())?;
        rustfmt_file(&bindings_file_path);
    }
    Ok(())
}

fn main() {
    let output_directory = PathBuf::from(env::var("OUT_DIR").unwrap());
    let shaders_directory = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("shaders");
    //let include_search_paths = vec![&shaders_directory];
    let bindings_file_name = "bindings.rs";

    compile_and_embed_shaders(
        &shaders_directory,
        &[&shaders_directory],
        &output_directory,
        bindings_file_name,
    )
    .unwrap();
}
