use crate::engine::Error;
use graal::{
    get_shader_compiler, shaderc,
    shaderc::{EnvVersion, ShaderKind, SpirvVersion, TargetEnv},
    BufferAccess, ImageAccess,
};
use spirv_reflect::types::ReflectTypeFlags;
use std::{collections::BTreeMap, path::Path};
use std::rc::Rc;
use graal::shaderc::OptimizationLevel;
use tracing::{error, warn};
use crate::shaders::bindings::EntryPoint;

type MacroDefinitions = BTreeMap<String, String>;


////////////////////////////////////////////////////////////////////////////////////////////////////

#[derive(Default)]
pub(super) struct CompilationInfo {
    pub(super) used_images: BTreeMap<String, ImageAccess>,
    pub(super) used_buffers: BTreeMap<String, BufferAccess>,
    pub(super) push_cst_size: usize,
}

pub(super) fn compile_shader_stage(
    file_path: &Path,
    global_defines: &BTreeMap<String, String>,
    defines: &BTreeMap<String, String>,
    shader_kind: ShaderKind,
    info: &mut CompilationInfo,
) -> Result<Vec<u32>, Error> {
    // path for diagnostics
    let display_path = file_path.display().to_string();

    // load source
    let source_content = match std::fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(error) => {
            return Err(Error::ShaderReadError {
                path: file_path.to_path_buf(),
                error: error.into(),
            });
        }
    };

    // determine include path
    // this is the current directory if the shader is embedded, otherwise it's the parent
    // directory of the shader file
    let mut base_include_path = std::env::current_dir().expect("failed to get current directory");
    if let Some(parent) = file_path.parent() {
        base_include_path = parent.to_path_buf();
    }

    // setup CompileOptions
    let mut options = shaderc::CompileOptions::new().unwrap();
    options.set_target_env(TargetEnv::Vulkan, EnvVersion::Vulkan1_3 as u32);
    options.set_target_spirv(SpirvVersion::V1_5);
    options.set_generate_debug_info();
    options.set_optimization_level(OptimizationLevel::Zero);
    options.set_auto_bind_uniforms(true);
    for (key, value) in global_defines.iter() {
        options.add_macro_definition(key, Some(value));
    }
    for (key, value) in defines.iter() {
        options.add_macro_definition(key, Some(value));
    }
    options.set_include_callback(move |requested_source, _type, _requesting_source, _include_depth| {
        let mut path = base_include_path.clone();
        path.push(requested_source);
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => return Err(e.to_string()),
        };
        Ok(shaderc::ResolvedInclude {
            resolved_name: path.display().to_string(),
            content,
        })
    });
    // add stage-specific macros
    match shader_kind {
        ShaderKind::Vertex => {
            options.add_macro_definition("__VERTEX__", None);
        }
        ShaderKind::Fragment => {
            options.add_macro_definition("__FRAGMENT__", None);
        }
        ShaderKind::Geometry => {
            options.add_macro_definition("__GEOMETRY__", None);
        }
        ShaderKind::Compute => {
            options.add_macro_definition("__COMPUTE__", None);
        }
        ShaderKind::TessControl => {
            options.add_macro_definition("__TESS_CONTROL__", None);
        }
        ShaderKind::TessEvaluation => {
            options.add_macro_definition("__TESS_EVAL__", None);
        }
        ShaderKind::Mesh => {
            options.add_macro_definition("__MESH__", None);
        }
        ShaderKind::Task => {
            options.add_macro_definition("__TASK__", None);
        }
        _ => {}
    }

    let compiler = get_shader_compiler();
    let compilation_artifact = match compiler.compile_into_spirv(&source_content, shader_kind, &display_path, "main", Some(&options)) {
        Ok(artifact) => artifact,
        Err(err) => {
            error!("failed to compile shader `{display_path}`: {err}");
            return Err(Rc::new(err).into());
        }
    };

    // dump warnings
    for warning in compilation_artifact.get_warning_messages().split('\n') {
        if !warning.is_empty() {
            warn!("`{display_path}`: {warning}");
        }
    }

    // dump compilation artifact to disk
    let stage_ext = match shader_kind {
        ShaderKind::Vertex => "vert",
        ShaderKind::Fragment => "frag",
        ShaderKind::Geometry => "geom",
        ShaderKind::Compute => "comp",
        ShaderKind::TessControl => "tesc",
        ShaderKind::TessEvaluation => "tese",
        ShaderKind::Mesh => "mesh",
        ShaderKind::Task => "task",
        _ => "unknown",
    };
    let dump_path = file_path.with_extension(format!("{stage_ext}.spv"));
    std::fs::write(&dump_path, &compilation_artifact.as_binary_u8()).unwrap();


    // remap resource bindings
    let module = spirv_reflect::create_shader_module(compilation_artifact.as_binary_u8()).unwrap();
    /*let descriptor_bindings = module.enumerate_descriptor_bindings(Some("main")).unwrap();
    for refl in descriptor_bindings.iter() {
        let ty = refl.descriptor_type;
        let name = refl.name.clone();
        if name.is_empty() {
            warn!("`{display_path}`: not binding anonymous {ty:?} resource");
            continue;
        }

        match refl.descriptor_type {
            ReflectDescriptorType::SampledImage | ReflectDescriptorType::StorageImage => {
                let Some(image) = self.images.get_mut(&ImageKey(name.as_str())) else {
                    warn!("`{display_path}`: unknown image `{name}`");
                    continue;
                };

                match refl.descriptor_type {
                    ReflectDescriptorType::SampledImage => {
                        module
                            .change_descriptor_binding_numbers(refl, image.texture_binding.binding, Some(image.texture_binding.set))
                            .unwrap();
                        image.inferred_usage |= ImageUsage::SAMPLED;
                    }
                    ReflectDescriptorType::StorageImage => {
                        module
                            .change_descriptor_binding_numbers(refl, image.storage_binding.binding, Some(image.storage_binding.set))
                            .unwrap();
                        image.inferred_usage |= ImageUsage::STORAGE;
                    }
                    _ => {}
                }
            }
            ReflectDescriptorType::UniformBuffer | ReflectDescriptorType::StorageBuffer => {
                let Some(buffer) = self.buffers.get_mut(&BufferKey(name.as_str())) else {
                    warn!("`{display_path}`: unknown buffer `{name}`");
                    continue;
                };

                match refl.descriptor_type {
                    ReflectDescriptorType::UniformBuffer => {
                        module
                            .change_descriptor_binding_numbers(refl, buffer.uniform_binding.binding, Some(buffer.uniform_binding.set))
                            .unwrap();
                        buffer.inferred_usage |= BufferUsage::UNIFORM_BUFFER;
                    }
                    ReflectDescriptorType::StorageBuffer => {
                        module
                            .change_descriptor_binding_numbers(refl, buffer.storage_binding.binding, Some(buffer.storage_binding.set))
                            .unwrap();
                        buffer.inferred_usage |= BufferUsage::STORAGE_BUFFER;
                    }
                    _ => {}
                }
            }
            ReflectDescriptorType::Sampler => {
                todo!()
            }
            ReflectDescriptorType::CombinedImageSampler => {
                todo!()
            }
            _ => {
                warn!("`{display_path}`: unsupported descriptor type {:?}", refl.descriptor_type);
                continue;
            }
        }
    }*/

    // reflect push constants
    let push_constants = module.enumerate_push_constant_blocks(Some("main")).unwrap();
    if push_constants.len() > 1 {
        warn!("`{display_path}`: multiple push constant blocks found; only the first one will be used");
    }


    if let Some(block) = push_constants.first() {
        if block.offset != 0 {
            warn!("`{display_path}`: push constant blocks at non-zero offset are not supported");
        } else {
            /*let mut add_constant = |name: &str, offset: u32, ty: UniformType| {
                if let Some(c) = info.push_cst_map.insert(name.to_string(), (offset, ty)) {
                    if c != (offset, ty) {
                        warn!("`{display_path}` push constant `{name}` redefined with different offset or type");
                    }
                }
            };


            for var in block.members.iter() {
                let Some(tydesc) = var.type_description.as_ref() else { continue };
                let offset = var.absolute_offset;

                //eprintln!("name: {:?} offset {:?} size {:?}", var.name, offset, var.size);

                if tydesc.type_flags.contains(ReflectTypeFlags::FLOAT) {
                    if tydesc.traits.numeric.scalar.width == 32 {
                        add_constant(&var.name, offset, UniformType::F32);
                    } else {
                        warn!("`{display_path}`: unsupported float width");
                        continue;
                    }
                } else if tydesc.type_flags.contains(ReflectTypeFlags::INT) {
                    if tydesc.traits.numeric.scalar.width == 32 {
                        add_constant(&var.name, offset, UniformType::I32);
                    } else {
                        warn!("`{display_path}`: unsupported float width");
                        continue;
                    }
                } else if tydesc.type_flags.contains(ReflectTypeFlags::VECTOR) {
                    match tydesc.traits.numeric.vector.component_count {
                        2 => add_constant(&var.name, offset, UniformType::Vec2),
                        3 => add_constant(&var.name, offset, UniformType::Vec3),
                        4 => add_constant(&var.name, offset, UniformType::Vec4),
                        _ => {
                            warn!("`{display_path}`: unsupported vector component count");
                            continue;
                        }
                    }
                } else if tydesc.type_flags.contains(ReflectTypeFlags::MATRIX) {
                    match (tydesc.traits.numeric.matrix.column_count, tydesc.traits.numeric.matrix.row_count) {
                        (2, 2) => add_constant(&var.name, offset, UniformType::Mat2),
                        (3, 3) => add_constant(&var.name, offset, UniformType::Mat3),
                        (4, 4) => add_constant(&var.name, offset, UniformType::Mat4),
                        _ => {
                            warn!("`{display_path}`: unsupported matrix shape");
                            continue;
                        }
                    }
                } else if tydesc.type_flags.contains(ReflectTypeFlags::STRUCT) && &tydesc.type_name == "samplerIndex" {
                    add_constant(&var.name, offset, UniformType::SamplerHandle);
                } else if tydesc.type_flags.contains(ReflectTypeFlags::STRUCT) && &tydesc.type_name == "texture2DIndex" {
                    add_constant(&var.name, offset, UniformType::Texture2DHandle);
                } else if tydesc.type_flags.contains(ReflectTypeFlags::STRUCT) && &tydesc.type_name == "image2DIndex" {
                    add_constant(&var.name, offset, UniformType::ImageHandle);
                } else if tydesc.type_flags.contains(ReflectTypeFlags::REF) {
                    add_constant(&var.name, offset, UniformType::DeviceAddress);
                } else {
                    //warn!("`{display_path}`: unsupported push constant type: `{} {};`", tydesc.type_name, var.name);
                    continue;
                }
            }*/

            info.push_cst_size = info.push_cst_size.max(block.size as usize);
        }
    }

    Ok(module.get_code())
}
