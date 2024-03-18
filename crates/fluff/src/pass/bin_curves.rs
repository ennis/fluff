use crate::pass::{BufferDescriptor, ImageDescriptor};
use graal::{Format, RasterizationState};

struct BinCurvesPass {}

const TILE_SIZE: u32 = 16;

impl BinCurvesPass {
    pub fn new() -> Self {
        Self {}
    }

    /*fn declare_pipeline(&self) -> PipelineDescriptor {
        PipelineDescriptor {
            unified_shader_file: Some("crates/fluff/src/pass/bin_curves.glsl"),
            defines: &[("TILE_SIZE", TILE_SIZE.to_string())],
            ..Default::default()
        }
    }

    fn declare_resources(&self, ctx: &PassResourceCtx) -> PassResources {
        let (width, height): (u32, u32) = ctx.screen_size();
        let tile_count_x = width.div_ceil(TILE_SIZE);
        let tile_count_y = height.div_ceil(TILE_SIZE);

        let resources = [ImageDescriptor {
            name: "u_tile_line_count",
            formats: &[Format::R32_SINT],
            width: Some(tile_count_x),
            height: Some(tile_count_y),
            ..Default::default()
        }];
        let buffers = [BufferDescriptor {
            name: "u_tile_buffer",
            count: Some((tile_count_x * tile_count_y) as u64),
            ..Default::default()
        }];
    }

    fn ui(&mut self, ctx: &PassCtx) -> bool {}*/
}
