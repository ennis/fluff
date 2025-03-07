use crate::scene::{Object, Scene3D};
use graal::{Buffer, DeviceAddress, RenderEncoder};
use std::ops::Range;

pub struct SceneRenderItem<'a> {
    /// Index buffer to bind.
    pub index_buffer: &'a Buffer<[u32]>,
    /// Range of indices to draw.
    pub index_range: Range<u32>,
    /// Pointer to position data on the GPU.
    pub position_ptr: DeviceAddress<[[f32; 3]]>,
    /// Pointer to normal data on the GPU.
    pub normal_ptr: Option<DeviceAddress<[[f32; 3]]>>,
    /// Pointer to UV (texture coordinates) data on the GPU.
    pub uv_ptr: Option<DeviceAddress<[[f32; 2]]>>,
    pub model_matrix: glam::Mat4,
}

pub trait SceneRenderVisitor {
    fn draw(&mut self, encoder: &mut RenderEncoder, render_item: &SceneRenderItem);
}

impl Scene3D {
    fn draw_recursive(
        &self,
        encoder: &mut RenderEncoder,
        obj: &Object,
        time: f64,
        visitor: &mut dyn SceneRenderVisitor,
    ) {
        // TODO
        let mut model_matrix = glam::Mat4::IDENTITY;

        if let Some(mesh) = &obj.geometry {
            let index_buffer = &mesh.indices;
            let index_range = 0..mesh.index_count;
            let position_ptr = mesh.positions.buffer.device_address();
            let normal_ptr = mesh.normals.as_ref().map(|attr| attr.buffer.device_address());
            let uv_ptr = mesh.uvs.as_ref().map(|attr| attr.buffer.device_address());

            encoder.reference_resource(index_buffer);
            //encoder.reference_resource(mesh.attributes[0].device_data.buffer());

            visitor.draw(
                encoder,
                &SceneRenderItem {
                    index_buffer,
                    index_range,
                    position_ptr,
                    normal_ptr,
                    uv_ptr,
                    model_matrix,
                },
            );
        }

        for child in &obj.children {
            self.draw_recursive(encoder, child, time, visitor);
        }
    }

    pub fn draw(&self, encoder: &mut RenderEncoder, time: f64, visitor: &mut dyn SceneRenderVisitor) {
        self.draw_recursive(encoder, &self.root, time, visitor);
    }
}
