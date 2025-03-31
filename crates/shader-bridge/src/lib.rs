mod compiler;
mod embed_shaders;
mod rustfmt;
mod syntax_bindgen;

pub use compiler::compile_shader_module;
pub use embed_shaders::compile_and_embed_shaders;
pub use rustfmt::rustfmt_file;
pub use syntax_bindgen::translate_slang_shared_decls;
