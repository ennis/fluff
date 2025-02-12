use shader_bridge::generate_shader_bridge;
use shader_bridge::reflect_slang_shaders;

fn main() {
    generate_shader_bridge("src/shaders/shared.rs", "shaders/shared.inc.glsl").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let output = format!("{}/bindings.rs", out_dir);
    reflect_slang_shaders("slang/", &output).unwrap();
}