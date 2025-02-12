//! Types & constants for interfacing between shader and application code.
//!
//! # Rant
//!
//! Currently, the definitions are duplicated and kept in sync by hand between
//! shaders & application. But _eventually_ we should be able to generate them automatically
//! by reflecting the shaders. Or maybe generate shader code from the definitions in this rust
//! module. For future reference, there are some potential approaches:
//!
//! 1. with a proc-macro, generate a GLSL header from Rust definitions, then compile the GLSL code,
//!    It's basically a shader compiler in a proc-macro, so I'm not too hot on this idea
//!    (it will kill IDE autocompletion if the shader has syntax errors).
//!    Also, we generate code on both sides (GLSL header and rust decls), the data structures are not
//!    defined next to where they're used in shaders... it's not perfect.
//!    Also, we can't evaluate anything other than literals in proc-macros, so stuff like `const TILE_SIZE = SOME_OTHER_CONSTANT * 64;`
//!    is out.
//!    Also, the macro doesn't know anything about types, so we must match stuff like `glam::Vec4` by name. Type aliases won't work.
//!
//! 2. Parse (the syntax of) the GLSL code in a proc-macro, then generate Rust code from the parsed AST.
//!    However, at the syntax stage, we don't have information about the interface of the shader
//!    (we need to analyze the usage of uniforms, attributes in the function call graph),
//!    so we can only generate code for types and constants, not shader interfaces.
//!
//! 3. Generate rust interface code from SPIR-V. This is the most promising approach, but SPIR-V
//!    modules **don't name the constants**, so we lose constants declared in shaders
//!    like `const int TILE_SIZE = 16;`.
//!    Also the source of truth for data types & constants is now in the shaders; not sure that's for the best?
//!
//! 4. Use slang and the declaration-reflection API that someone's working on (https://github.com/shader-slang/slang/issues/4617).
//!    It's not ready yet, and there's no time frame.
//!
//! In conclusion, the shader ecosystem is a dumpster fire. There's absolutely no coordinated effort
//! to improve the developer experience across the whole pipeline.

pub mod types;
pub mod shared;


