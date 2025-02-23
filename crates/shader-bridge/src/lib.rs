mod compiler;
mod embed_shaders;
mod syntax_bindgen;
mod rustfmt;

pub use syntax_bindgen::translate_slang_shared_decls;
pub use embed_shaders::compile_and_embed_shaders;
pub use rustfmt::rustfmt_file;
pub use compiler::compile_shader_module;