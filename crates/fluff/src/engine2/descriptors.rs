use graal::{vk, BufferRangeUntyped, Device, ImageView, Sampler, GpuResource, RenderEncoder, ComputeEncoder, DescriptorSetLayout};
use std::{
    ffi::c_void,
    ptr,
    sync::{
        atomic::{AtomicU64, Ordering},
    },
};

pub(super) const STATIC_UBO_COUNT: u32 = 16;
pub(super) const STATIC_SSBO_COUNT: u32 = 32;
pub(super) const STATIC_TEX_COUNT: u32 = 32;
pub(super) const STATIC_IMG_COUNT: u32 = 32;
pub(super) const STATIC_SAMPLERS_COUNT: u32 = 32;
pub(super) const MAX_INDEXED_SSBO_COUNT: u32 = 4096;
pub(super) const MAX_INDEXED_TEX_COUNT: u32 = 4096;
pub(super) const MAX_INDEXED_IMG_COUNT: u32 = 4096;

pub(super) const UBO_SET: u32 = 0;
pub(super) const SSBO_SET: u32 = 1;
pub(super) const TEX_SET: u32 = 2;
pub(super) const IMG_SET: u32 = 3;
pub(super) const SAMPLERS_SET: u32 = 4;

pub(crate) struct UniversalPipelineLayout {
    pub(crate) ubo: DescriptorSetLayout,
    pub(crate) ssbo: DescriptorSetLayout,
    pub(crate) texture: DescriptorSetLayout,
    pub(crate) image: DescriptorSetLayout,
    pub(crate) sampler: DescriptorSetLayout,
}

impl UniversalPipelineLayout {
    pub(crate) fn new(device: &Device) -> UniversalPipelineLayout {
        let sets = [
            (vk::DescriptorType::UNIFORM_BUFFER, STATIC_UBO_COUNT, None),
            (vk::DescriptorType::STORAGE_BUFFER, STATIC_SSBO_COUNT, Some(MAX_INDEXED_SSBO_COUNT)),
            (vk::DescriptorType::SAMPLED_IMAGE, STATIC_TEX_COUNT, Some(MAX_INDEXED_TEX_COUNT)),
            (vk::DescriptorType::STORAGE_IMAGE, STATIC_IMG_COUNT, Some(MAX_INDEXED_IMG_COUNT)),
            (vk::DescriptorType::SAMPLER, STATIC_SAMPLERS_COUNT, None),
        ];

        let mut layouts = vec![];

        for (descriptor_type, binding_count, max_indexed_count) in sets {
            let mut bindings = vec![];
            let mut flags = vec![];

            bindings.extend((0..binding_count).map(|i| vk::DescriptorSetLayoutBinding {
                binding: i,
                descriptor_type,
                descriptor_count: 1,
                stage_flags: vk::ShaderStageFlags::ALL,
                p_immutable_samplers: ptr::null(),
            }));
            flags.extend((0..binding_count).map(|_| vk::DescriptorBindingFlags::empty()));

            if let Some(max_indexed_count) = max_indexed_count {
                bindings.push(vk::DescriptorSetLayoutBinding {
                    binding: bindings.len() as u32,
                    descriptor_type,
                    descriptor_count: max_indexed_count,
                    stage_flags: vk::ShaderStageFlags::ALL,
                    p_immutable_samplers: ptr::null(),
                });
                flags.push(vk::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT);
            }

            let dslbfci = vk::DescriptorSetLayoutBindingFlagsCreateInfo {
                binding_count: flags.len() as u32,
                p_binding_flags: flags.as_ptr(),
                ..Default::default()
            };

            let dslci = vk::DescriptorSetLayoutCreateInfo {
                p_next: &dslbfci as *const _ as *const c_void,
                flags: Default::default(),
                binding_count: bindings.len() as u32,
                p_bindings: bindings.as_ptr(),
                ..Default::default()
            };

            let layout = unsafe {
                let handle = device
                    .create_descriptor_set_layout(&dslci, None)
                    .expect("failed to create descriptor set layout");
                device.create_descriptor_set_layout_from_handle(
                    handle
                )
            };
            layouts.push(Some(layout));
        }

        UniversalPipelineLayout {
            ubo: layouts[0].take().unwrap(),
            ssbo: layouts[1].take().unwrap(),
            texture: layouts[2].take().unwrap(),
            image: layouts[3].take().unwrap(),
            sampler: layouts[4].take().unwrap(),
        }
    }
}


#[derive(Debug)]
pub struct ResourceDescriptorsBuilder {
    pub ubo: Vec<(u32, BufferRangeUntyped)>,
    pub ssbo: Vec<(u32, BufferRangeUntyped)>,
    pub tex: Vec<(u32, ImageView)>,
    pub img: Vec<(u32, ImageView)>,
    pub samplers: Vec<(u32, Sampler)>,

    pub indexed_ssbo: Vec<BufferRangeUntyped>,
    pub indexed_tex: Vec<ImageView>,
    pub indexed_img: Vec<ImageView>,
}

impl ResourceDescriptorsBuilder {
    pub fn new() -> Self {
        Self {
            ubo: Vec::new(),
            ssbo: Vec::new(),
            tex: Vec::new(),
            img: Vec::new(),
            samplers: Vec::new(),
            indexed_ssbo: Vec::new(),
            indexed_tex: Vec::new(),
            indexed_img: Vec::new(),
        }
    }

    pub fn bind_uniform_buffer(&mut self, binding: u32, buffer: BufferRangeUntyped) -> (u32, u32) {
        self.ubo.push((binding, buffer));
        (UBO_SET, binding)
    }

    pub fn bind_storage_buffer(&mut self, binding: u32, buffer: BufferRangeUntyped) -> (u32, u32) {
        self.ssbo.push((binding, buffer));
        (SSBO_SET, binding)
    }

    pub fn bind_sampled_image(&mut self, binding: u32, image: ImageView) -> (u32, u32) {
        self.tex.push((binding, image));
        (TEX_SET, binding)
    }

    pub fn bind_storage_image(&mut self, binding: u32, image: ImageView) -> (u32, u32) {
        self.img.push((binding, image));
        (IMG_SET, binding)
    }

    pub fn bind_sampler(&mut self, binding: u32, sampler: Sampler) -> (u32, u32) {
        self.samplers.push((binding, sampler));
        (SAMPLERS_SET, binding)
    }

    pub fn push_indexed_storage_buffer(&mut self, buffer: BufferRangeUntyped) -> usize {
        self.indexed_ssbo.push(buffer);
        self.indexed_ssbo.len() - 1
    }

    pub fn push_indexed_sampled_image(&mut self, image: ImageView) -> usize {
        self.indexed_tex.push(image);
        self.indexed_tex.len() - 1
    }

    pub fn push_indexed_storage_image(&mut self, image: ImageView) -> usize {
        self.indexed_img.push(image);
        self.indexed_img.len() - 1
    }

    pub fn build(mut self, device: &Device, layout: &UniversalPipelineLayout) -> ResourceDescriptors {
        #[rustfmt::skip]
        let pool_create_info = vk::DescriptorPoolCreateInfo {
            max_sets: 1,
            pool_size_count: 5,
            p_pool_sizes: [
                vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER, descriptor_count: STATIC_UBO_COUNT },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_BUFFER, descriptor_count: STATIC_SSBO_COUNT + self.indexed_ssbo.len() as u32 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: STATIC_TEX_COUNT + self.indexed_tex.len() as u32 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: STATIC_IMG_COUNT + self.indexed_img.len() as u32 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: STATIC_SAMPLERS_COUNT + self.indexed_tex.len() as u32 },
            ].as_ptr(),
            ..Default::default()
        };


        unsafe {
            let pool = device
                .create_descriptor_pool(&pool_create_info, None)
                .expect("failed to create descriptor pool");
            let sets = device
                .allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo {
                    descriptor_pool: pool,
                    descriptor_set_count: 5,
                    p_set_layouts: [
                        layout.ubo.handle,
                        layout.ssbo.handle,
                        layout.texture.handle,
                        layout.image.handle,
                        layout.sampler.handle,
                    ].as_ptr(),
                    ..Default::default()
                })
                .expect("failed to allocate descriptor sets");

            let mut writes = Vec::new();
            for (binding, buffer) in self.ubo.iter() {
                writes.push(vk::WriteDescriptorSet {
                    dst_set: sets[UBO_SET as usize],
                    dst_binding: *binding,
                    dst_array_element: 0,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::UNIFORM_BUFFER,
                    p_buffer_info: &vk::DescriptorBufferInfo {
                        buffer: buffer.handle(),
                        offset: buffer.offset(),
                        range: buffer.size(),
                    },
                    ..Default::default()
                });
            }

            for (binding, buffer) in self.ssbo.iter() {
                writes.push(vk::WriteDescriptorSet {
                    dst_set: sets[SSBO_SET as usize],
                    dst_binding: *binding,
                    dst_array_element: 0,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::STORAGE_BUFFER,
                    p_buffer_info: &vk::DescriptorBufferInfo {
                        buffer: buffer.handle(),
                        offset: buffer.offset(),
                        range: buffer.size(),
                    },
                    ..Default::default()
                });
            }

            for (binding, image) in self.tex.iter() {
                writes.push(vk::WriteDescriptorSet {
                    dst_set: sets[TEX_SET as usize],
                    dst_binding: *binding,
                    dst_array_element: 0,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                    p_image_info: &vk::DescriptorImageInfo {
                        sampler: vk::Sampler::null(),
                        image_view: image.handle(),
                        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    },
                    ..Default::default()
                });
            }

            for (binding, image) in self.img.iter() {
                writes.push(vk::WriteDescriptorSet {
                    dst_set: sets[IMG_SET as usize],
                    dst_binding: *binding,
                    dst_array_element: 0,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
                    p_image_info: &vk::DescriptorImageInfo {
                        sampler: vk::Sampler::null(),
                        image_view: image.handle(),
                        image_layout: vk::ImageLayout::GENERAL,
                    },
                    ..Default::default()
                });
            }

            for (binding, sampler) in self.samplers.iter() {
                writes.push(vk::WriteDescriptorSet {
                    dst_set: sets[SAMPLERS_SET as usize],
                    dst_binding: *binding,
                    dst_array_element: 0,
                    descriptor_count: 1,
                    descriptor_type: vk::DescriptorType::SAMPLER,
                    p_image_info: &vk::DescriptorImageInfo {
                        sampler: sampler.handle(),
                        image_view: vk::ImageView::null(),
                        image_layout: vk::ImageLayout::UNDEFINED,
                    },
                    ..Default::default()
                });
            }

            device.update_descriptor_sets(&writes, &[]);

            ResourceDescriptors {
                device: device.clone(),
                pool,
                sets,
                last_submission_index: AtomicU64::new(0),
                builder: self,
            }
        }
    }
}

/// Argument buffer.
///
/// # Guidelines
///
/// Should be used for long-lived resources. It's fairly expensive to create.
/// The intended usage is to create it once, stuffing all resources accessed by all shaders into it,
/// and never touch it again.
///
/// If you need frequently changing descriptors, use push descriptors instead.
#[derive(Debug)]
pub struct ResourceDescriptors {
    device: Device,
    /// Pool from which the descriptor sets are allocated.
    /// Each argument buffer has its own pool. There shouldn't be a lot of argument buffers anyway.
    pool: vk::DescriptorPool,
    sets: Vec<vk::DescriptorSet>,
    /// Last submission that used the descriptor sets.
    last_submission_index: AtomicU64,
    /// The builder used to create this argument buffer.
    /// We keep it so that the resources referenced by the descriptor sets are kept alive.
    builder: ResourceDescriptorsBuilder,
}


impl Drop for ResourceDescriptors {
    fn drop(&mut self) {
        unsafe {
            let device = self.device.clone();
            self.device.call_later(self.last_submission_index.load(Ordering::Relaxed), move || {
                device.destroy_descriptor_pool(self.pool, None);
            });
        }
    }
}

impl GpuResource for ResourceDescriptors {
    fn set_last_submission_index(&self, submission_index: u64) {
        self.last_submission_index.store(submission_index, Ordering::Relaxed);
    }
}

impl ResourceDescriptors {
    pub(super) fn bind_render(&self, enc: &mut RenderEncoder) {
        enc.reference_resource(self);
        unsafe {
            enc.bind_descriptor_set(UBO_SET, self.sets[UBO_SET as usize]);
            enc.bind_descriptor_set(SSBO_SET, self.sets[SSBO_SET as usize]);
            enc.bind_descriptor_set(TEX_SET, self.sets[TEX_SET as usize]);
            enc.bind_descriptor_set(IMG_SET, self.sets[IMG_SET as usize]);
            enc.bind_descriptor_set(SAMPLERS_SET, self.sets[SAMPLERS_SET as usize]);
        }
    }

    pub(super) fn bind_compute(&self, enc: &mut ComputeEncoder) {
        enc.reference_resource(self);
        unsafe {
            enc.bind_descriptor_set(UBO_SET, self.sets[UBO_SET as usize]);
            enc.bind_descriptor_set(SSBO_SET, self.sets[SSBO_SET as usize]);
            enc.bind_descriptor_set(TEX_SET, self.sets[TEX_SET as usize]);
            enc.bind_descriptor_set(IMG_SET, self.sets[IMG_SET as usize]);
            enc.bind_descriptor_set(SAMPLERS_SET, self.sets[SAMPLERS_SET as usize]);
        }
    }
}
