use crate::shaders::compile_and_embed_shaders;
use crate::syntax_bindgen::translate_slang_shared_decls;
use shader_bridge::generate_shader_bridge;
use slang::Downcast;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::env;
use std::io::Write;

#[path = "build/syntax_bindgen.rs"]
mod syntax_bindgen;
#[path = "build/shaders.rs"]
mod shaders;


/// Gets the rustfmt path to rustfmt the generated bindings.
fn rustfmt_path<'a>() -> PathBuf {
    if let Ok(rustfmt) = env::var("RUSTFMT") {
        rustfmt.into()
    } else {
        // just assume that it is in path
        "rustfmt".into()
    }
}

/// Runs rustfmt on a file.
fn rustfmt_file(path: &Path) {
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

const SHADERS_DIR: &str = "slang/";
const SLANG_SHARED: &str = "slang/shared.slang";


fn main() {
    //generate_shader_bridge("src/shaders/shared.rs", "shaders/shared.inc.glsl").unwrap();

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let shader_bindings = out_dir.join("bindings.rs");

    {
        let mut output = File::create(&shader_bindings).unwrap();
        
        writeln!(&mut output, "/* This file is generated by build.rs. Do not edit this file. */").unwrap();

        // Translate `slang/shared.slang`
        let slang_shared = Path::new(SLANG_SHARED);
        translate_slang_shared_decls(slang_shared, &mut output);

        // Compile and embed shaders
        let shaders_dir = Path::new(SHADERS_DIR);
        compile_and_embed_shaders(shaders_dir, &[], &out_dir, &mut output);
    }
    
    rustfmt_file(&shader_bindings);
    

    //generate_slang_rust_bindings("slang/", &[], &SlangRustBindingOptions { slang_declarations_file: Some("shared.slang"), output: &output }).unwrap();
}
