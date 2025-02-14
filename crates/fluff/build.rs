use shader_bridge::{generate_shader_bridge, generate_slang_rust_bindings, SlangRustBindingOptions};

fn main() {
    generate_shader_bridge("src/shaders/shared.rs", "shaders/shared.inc.glsl").unwrap();

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let output = format!("{}/bindings.rs", out_dir);

    generate_slang_rust_bindings("slang/", &[], &SlangRustBindingOptions { slang_declarations_file: Some("shared.slang"), output: &output }).unwrap();
}
