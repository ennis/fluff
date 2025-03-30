use crate::camera_control::Camera;
use crate::gpu::PrimitiveRenderPipelineDesc;
use crate::scene::{Scene3D, SceneRenderItem, SceneRenderVisitor};
use crate::shaders::SceneParams;
use crate::{gpu, shaders};
use glam::uvec2;
use graal::prelude::CommandStreamExt;
use graal::{
    vk, ColorAttachment, ColorBlendEquation, ColorTargetState, CommandStream, DepthStencilAttachment,
    DepthStencilState, DeviceAddress, Format, RenderEncoder, RenderPassInfo, StencilState,
};
use std::mem;

pub struct DebugRenderVisitor<'a> {
    pub camera: &'a Camera,
    pub scene_params: DeviceAddress<SceneParams>,
}

impl SceneRenderVisitor for DebugRenderVisitor<'_> {
    fn draw(&mut self, encoder: &mut RenderEncoder, render_item: &SceneRenderItem) {
        // SAFETY: same layout
        let indices = unsafe { mem::transmute(render_item.index_buffer.device_address()) };
        let position = unsafe { mem::transmute(render_item.position_ptr) };
        let normal = render_item
            .normal_ptr
            .map(|ptr| unsafe { mem::transmute(ptr) })
            .unwrap_or(DeviceAddress::NULL);
        let texcoord = render_item
            .uv_ptr
            .map(|ptr| unsafe { mem::transmute(ptr) })
            .unwrap_or(DeviceAddress::NULL);

        let model_matrix = render_item.model_matrix.to_cols_array_2d();
        //let model_normal_matrix = render_item.model_matrix.inverse().transpose().to_cols_array_2d();
        //let view_matrix = self.camera.view.to_cols_array_2d();
        //let projection_matrix = self.camera.projection.to_cols_array_2d();

        encoder.push_constants(&shaders::GeometryData {
            scene_params: self.scene_params,
            indices,
            position,
            normal,
            texcoord,
            color: DeviceAddress::NULL,
            model_matrix,
        });
        encoder.set_primitive_topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        encoder.draw(render_item.index_range.clone(), 0..1);
    }
}

pub fn debug_render_scene(
    cmd: &mut CommandStream,
    scene: &Scene3D,
    camera: &Camera,
    time: f64,
    color_target: &graal::ImageView,
    depth_target: &graal::ImageView,
) {
    let width = color_target.width();
    let height = color_target.height();

    let geometry_pipeline = gpu::create_primitive_pipeline(
        "geometry",
        &PrimitiveRenderPipelineDesc {
            vertex_shader: shaders::GEOMETRY_VERTEX_SHADER,
            fragment_shader: shaders::GEOMETRY_FRAGMENT_SHADER,
            color_targets: vec![ColorTargetState {
                format: color_target.format(),
                blend_equation: Some(ColorBlendEquation::ALPHA_BLENDING),
                ..Default::default()
            }],
            rasterization_state: Default::default(),
            depth_stencil_state: Some(DepthStencilState {
                format: Format::D32_SFLOAT,
                depth_write_enable: true,
                depth_compare_op: vk::CompareOp::LESS_OR_EQUAL,
                stencil_state: StencilState::default(),
            }),
            multisample_state: Default::default(),
        },
    )
    .unwrap();

    let scene_params = cmd.upload_temporary(&shaders::SceneParams {
        view_matrix: camera.view.to_cols_array_2d(),
        projection_matrix: camera.projection.to_cols_array_2d(),
        view_projection_matrix: camera.view_projection().to_cols_array_2d(),
        eye: camera.eye().as_vec3(),
        // TODO frustum parameters
        near_clip: camera.frustum.near_plane,
        far_clip: camera.frustum.far_plane,
        left: 0.0,
        right: 0.0,
        top: 0.0,
        bottom: 0.0,
        viewport_size: uvec2(width, height),
        cursor_pos: Default::default(),
        time: time as f32,
    });

    let mut v = DebugRenderVisitor {
        camera: &camera,
        scene_params,
    };
    let mut encoder = cmd.begin_rendering(RenderPassInfo {
        color_attachments: &[ColorAttachment {
            image_view: color_target,
            clear_value: Some([0.0, 0.0, 0.0, 1.0]),
        }],
        depth_stencil_attachment: Some(DepthStencilAttachment {
            image_view: depth_target,
            depth_clear_value: Some(1.0),
            stencil_clear_value: None,
        }),
    });
    encoder.set_viewport(0.0, height as f32, width as f32, -(height as f32), 0.0, 1.0);
    encoder.set_scissor(0, 0, width, height);
    encoder.bind_graphics_pipeline(&geometry_pipeline);
    scene.draw(&mut encoder, 0.0, &mut v);
}
