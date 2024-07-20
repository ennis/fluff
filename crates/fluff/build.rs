use shader_bridge::generate_shader_bridge;

fn main() {
    generate_shader_bridge("src/shaders/shared.rs", "shaders/shared.inc.glsl").unwrap();
}