use std::{error::Error, fs};

use crate::parse::parse_file;

mod gen;
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


pub fn reflect_slang_shaders(shader_directory: &str, output_path: &str) -> Result<(), Box<dyn Error>> {

    let mut ctx = reflect::Ctx::new(&reflect::CtxOptions {
        search_paths: &[shader_directory.as_ref()],
    })?;

    let tokens = ctx.reflect()?;
    // write tokens to file
    let output = tokens.to_string();
    fs::write(output_path, output)?;
    // run rustfmt
    utils::rustfmt_file(output_path.as_ref());
    
    drop(ctx);

    Ok(())
}


#[cfg(test)]
mod test {
    use super::*;

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
    fn slang_shader() {
        env_logger::init();
        let _ = reflect_slang_shaders("../fluff/slang/", "src/shaders2.rs");
    }
}
