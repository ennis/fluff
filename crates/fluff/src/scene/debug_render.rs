use std::mem;
use graal::{vk, CommandStream, DeviceAddress, RenderEncoder};
use crate::camera_control::Camera;
use crate::scene::{Scene3D, SceneRenderItem, SceneRenderVisitor};
use crate::shaders;
use crate::shaders::SceneParams;

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
    // TODO
    //let mut visitor = DebugRenderVisitor { camera, scene_params };
    //scene.render(&mut visitor, encoder);
}