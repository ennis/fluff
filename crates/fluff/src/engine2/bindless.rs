//! Bindless pipeline layout & descriptor set creation
use graal::{vk, BufferRangeUntyped, Device, ImageView, Sampler, GpuResource, RenderEncoder, ComputeEncoder, DescriptorSetLayout};
use std::{ffi::c_void, mem, ptr, sync::{
    atomic::{AtomicU64, Ordering},
}};
use graal::vk::Handle;
use tracing::debug;
use crate::engine2::Image;

pub(super) const MAX_INDEXED_TEX_COUNT: u32 = 4096;
pub(super) const MAX_INDEXED_SAMPLER_COUNT: u32 = 4096;

pub(super) const TEX_SET: u32 = 0;
pub(super) const IMG_SET: u32 = 1;
pub(super) const SAMPLERS_SET: u32 = 2;

pub(crate) struct BindlessLayout {
    pub(crate) textures: DescriptorSetLayout,
    pub(crate) images: DescriptorSetLayout,
    pub(crate) samplers: DescriptorSetLayout,
}

impl BindlessLayout {
    pub(crate) fn new(device: &Device) -> BindlessLayout {
        let sets = [
            (vk::DescriptorType::SAMPLED_IMAGE, MAX_INDEXED_TEX_COUNT),
            (vk::DescriptorType::STORAGE_IMAGE, MAX_INDEXED_TEX_COUNT),
            (vk::DescriptorType::SAMPLER, MAX_INDEXED_SAMPLER_COUNT),
        ];

        let mut layouts = vec![];

        for (descriptor_type, max_count) in sets {
            let mut bindings = vec![];
            let mut flags = vec![];

            bindings.push(vk::DescriptorSetLayoutBinding {
                binding: bindings.len() as u32,
                descriptor_type,
                descriptor_count: max_count,
                stage_flags: vk::ShaderStageFlags::ALL,
                p_immutable_samplers: ptr::null(),
            });
            flags.push(vk::DescriptorBindingFlags::VARIABLE_DESCRIPTOR_COUNT);

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

        BindlessLayout {
            textures: layouts[0].take().unwrap(),
            images: layouts[1].take().unwrap(),
            samplers: layouts[2].take().unwrap(),
        }
    }

    pub fn create_descriptors(&self,
                              device: &Device,
                              images: &[Image],
                              samplers: &[Sampler]) -> ResourceDescriptors
    {
        let mut images_count = images.len() as u32;
        let mut samplers_count = samplers.len() as u32;
        // vkCreateDescriptorPool doesn't like zero-sized pools
        images_count = images_count.max(1);
        samplers_count = samplers_count.max(1);

        #[rustfmt::skip]
        let pool_sizes = [
            vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: images_count },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: images_count },
            vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: samplers_count },
        ];
        let pool_create_info = vk::DescriptorPoolCreateInfo {
            max_sets: 3,
            pool_size_count: 3,
            p_pool_sizes: pool_sizes.as_ptr(),
            ..Default::default()
        };

        let pool = unsafe {
            device
                .create_descriptor_pool(&pool_create_info, None)
                .expect("failed to create descriptor pool")
        };


        let descriptor_counts = [images_count, images_count, samplers_count];
        let set_counts = vk::DescriptorSetVariableDescriptorCountAllocateInfo {
            descriptor_set_count: 3,
            p_descriptor_counts: descriptor_counts.as_ptr(),
            ..Default::default()
        };

        let sets = unsafe {
            device
                .allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo {
                    p_next: &set_counts as *const _ as *const c_void,
                    descriptor_pool: pool,
                    descriptor_set_count: 3,
                    p_set_layouts: [
                        self.textures.handle,
                        self.images.handle,
                        self.samplers.handle,
                    ].as_ptr(),
                    ..Default::default()
                })
                .expect("failed to allocate descriptor sets")
        };

        unsafe {
            device.set_object_name(pool, "Bindless descriptors pool");
            device.set_object_name(sets[TEX_SET as usize], "Bindless textures");
            device.set_object_name(sets[IMG_SET as usize], "Bindless images");
            device.set_object_name(sets[SAMPLERS_SET as usize], "Bindless samplers");
        }

        let mut descriptor_image_infos = Vec::new();

        if !images.is_empty() {
            // Write texture descriptors
            for img in images {
                descriptor_image_infos.push(vk::DescriptorImageInfo {
                    sampler: vk::Sampler::null(),
                    image_view: img.view().handle(),
                    image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                });
            }

            unsafe {
                device.update_descriptor_sets(&[vk::WriteDescriptorSet {
                    dst_set: sets[TEX_SET as usize],
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptor_count: descriptor_image_infos.len() as u32,
                    descriptor_type: vk::DescriptorType::SAMPLED_IMAGE,
                    p_image_info: descriptor_image_infos.as_ptr(),
                    ..Default::default()
                }], &[]);
            }

            // Write image descriptors
            descriptor_image_infos.clear();
            for img in images {
                descriptor_image_infos.push(vk::DescriptorImageInfo {
                    sampler: vk::Sampler::null(),
                    image_view: img.view().handle(),
                    image_layout: vk::ImageLayout::GENERAL,
                });
            }

            unsafe {
                device.update_descriptor_sets(&[vk::WriteDescriptorSet {
                    dst_set: sets[IMG_SET as usize],
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptor_count: descriptor_image_infos.len() as u32,
                    descriptor_type: vk::DescriptorType::STORAGE_IMAGE,
                    p_image_info: descriptor_image_infos.as_ptr(),
                    ..Default::default()
                }], &[]);
            }
        }

        if !samplers.is_empty() {
            // Write sampler descriptors
            descriptor_image_infos.clear();
            for sampler in samplers {
                descriptor_image_infos.push(vk::DescriptorImageInfo {
                    sampler: sampler.handle(),
                    image_view: vk::ImageView::null(),
                    image_layout: vk::ImageLayout::UNDEFINED,
                });
            }
            unsafe {
                device.update_descriptor_sets(&[vk::WriteDescriptorSet {
                    dst_set: sets[SAMPLERS_SET as usize],
                    dst_binding: 0,
                    dst_array_element: 0,
                    descriptor_count: descriptor_image_infos.len() as u32,
                    descriptor_type: vk::DescriptorType::SAMPLER,
                    p_image_info: descriptor_image_infos.as_ptr(),
                    ..Default::default()
                }], &[]);
            }
        }

        ResourceDescriptors {
            device: device.clone(),
            pool,
            sets,
            last_submission_index: AtomicU64::new(0),
            images: images.iter().map(|img| img.view()).collect(),
            samplers: samplers.to_vec(),
        }
    }
}


/// Bindless resource descriptors.
#[derive(Debug)]
pub struct ResourceDescriptors {
    device: Device,
    /// Pool from which the descriptor sets are allocated.
    /// Each argument buffer has its own pool. There shouldn't be a lot of argument buffers anyway.
    pool: vk::DescriptorPool,
    sets: Vec<vk::DescriptorSet>,
    /// Last submission that used the descriptor sets.
    last_submission_index: AtomicU64,
    /// The resources contained in the descriptors.
    /// This is to ensure that the resources are not dropped while the descriptor sets are still in use.
    images: Vec<ImageView>,
    samplers: Vec<Sampler>,
}


impl Drop for ResourceDescriptors {
    fn drop(&mut self) {
        unsafe {
            let device = self.device.clone();
            let pool = self.pool;
            // FIXME: we move the references to images & samplers into the closure
            // so that their lifetime is extended to the lifetime of these descriptors.
            // This effectively extends the lifetime of image views (and images, transitively)
            // and images until the command buffer using the descriptor has completed.
            // However this is somewhat inelegant. It might be better to just
            // set the last submission index for each resource.
            let mut images = mem::take(&mut self.images);
            let mut samplers = mem::take(&mut self.samplers);
            let last_submission_index = self.last_submission_index.load(Ordering::Relaxed);
            self.device.call_later(last_submission_index, move || {
                debug!("ResourceDescriptors for submission {last_submission_index}");
                device.destroy_descriptor_pool(pool, None);
                // release images & samplers now
                let _images = mem::take(&mut images);
                let _samplers = mem::take(&mut samplers);
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
            enc.bind_descriptor_set(TEX_SET, self.sets[TEX_SET as usize]);
            enc.bind_descriptor_set(IMG_SET, self.sets[IMG_SET as usize]);
            enc.bind_descriptor_set(SAMPLERS_SET, self.sets[SAMPLERS_SET as usize]);
        }
    }

    pub(super) fn bind_compute(&self, enc: &mut ComputeEncoder) {
        enc.reference_resource(self);
        unsafe {
            enc.bind_descriptor_set(TEX_SET, self.sets[TEX_SET as usize]);
            enc.bind_descriptor_set(IMG_SET, self.sets[IMG_SET as usize]);
            enc.bind_descriptor_set(SAMPLERS_SET, self.sets[SAMPLERS_SET as usize]);
        }
    }
}

/// Extension trait for binding resource descriptors to render & compute encoders.
pub(crate) trait ResourceDescriptorBindExt {
    fn bind_resource_descriptors(&mut self, resource_descriptors: &ResourceDescriptors);
}

impl ResourceDescriptorBindExt for RenderEncoder<'_> {
    fn bind_resource_descriptors(&mut self, resource_descriptors: &ResourceDescriptors) {
        resource_descriptors.bind_render(self);
    }
}

impl ResourceDescriptorBindExt for ComputeEncoder<'_> {
    fn bind_resource_descriptors(&mut self, resource_descriptors: &ResourceDescriptors) {
        resource_descriptors.bind_compute(self);
    }
}