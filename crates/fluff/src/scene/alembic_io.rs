use crate::gpu;
use crate::scene::{Attribute, Mesh3D, Object, Scene3D};
use alembic_ogawa::geom::{GeomParam, GeometryScope, MeshTopologyVariance, PolyMesh, XForm};
use alembic_ogawa::{DataType, ObjectReader, TimeSample, TypedArrayPropertyReader};
use anyhow::bail;
use graal::util::DeviceExt;
use graal::{BufferUsage, MemoryLocation, RcDevice};
use std::mem::MaybeUninit;
use std::path::Path;
use tracing::{debug, info, warn};

fn triangulate_indices(face_counts: &[i32], indices: &[i32], output: &mut [MaybeUninit<u32>]) {
    let mut ii = 0;
    let mut oi = 0;
    for face_count in face_counts.iter() {
        let base_index = indices[ii];
        for j in (ii + 2)..(ii + *face_count as usize) {
            let a = indices[j - 1];
            let b = indices[j];
            output[oi].write(base_index as u32);
            output[oi + 1].write(a as u32);
            output[oi + 2].write(b as u32);
            oi += 3;
        }
        ii += *face_count as usize;
    }
}

fn read_attribute_samples<T: DataType + Copy>(
    device: &RcDevice,
    expected_count: usize,
    attribute: &TypedArrayPropertyReader<T>,
) -> Result<Attribute<[T]>, anyhow::Error> {
    let sample_count = attribute.sample_count();
    let mut buffer = device.create_array_buffer::<T>(
        BufferUsage::STORAGE_BUFFER,
        MemoryLocation::CpuToGpu,
        sample_count * expected_count,
    );

    // SAFETY: TODO
    let slice = unsafe { buffer.as_mut_slice() };
    let mut time_samples = vec![];
    for i in 0..sample_count {
        assert_eq!(&attribute.dimensions(i)[..], &[expected_count]);
        time_samples.push(attribute.time_sampling().get_sample_time(i).unwrap());
        attribute.read_sample_into(i, &mut slice[i * expected_count..])?;
    }

    Ok(Attribute { time_samples, buffer })
}

fn read_geom_param<T: DataType + Copy>(
    device: &RcDevice,
    face_vertex_count: usize,
    gp: &GeomParam<T>,
) -> Result<Attribute<[T]>, anyhow::Error> {
    // number of elements
    let elem_count = match gp.scope {
        GeometryScope::Constant => 1,
        GeometryScope::Uniform => 1,
        GeometryScope::Varying => face_vertex_count,
        GeometryScope::Vertex => face_vertex_count,
        GeometryScope::FaceVarying => face_vertex_count,
        GeometryScope::Unknown => {
            return Err(anyhow::anyhow!("unknown geometry scope"));
        }
    };
    assert_eq!(gp.values.dimensions(0)[0], elem_count);

    // number of unique samples
    let unique_sample_count = if let Some(ref indices) = gp.indices {
        debug!("indexed geom param: {} unique samples", indices.dimensions(0)[0]);
        indices.dimensions(0)[0]
    } else {
        debug!("non-indexed geom param: {} samples", gp.values.sample_count());
        gp.values.sample_count()
    };

    let mut buffer = device.create_array_buffer::<T>(
        BufferUsage::STORAGE_BUFFER,
        MemoryLocation::CpuToGpu,
        unique_sample_count * elem_count,
    );

    // SAFETY: TODO
    let slice = unsafe { buffer.as_mut_slice() };
    let mut time_samples = vec![];
    for i in 0..unique_sample_count {
        assert_eq!(&gp.values.dimensions(i)[..], &[elem_count]);
        time_samples.push(gp.values.time_sampling().get_sample_time(i).unwrap());
        gp.values.read_sample_into(i, &mut slice[i * elem_count..])?;
    }

    Ok(Attribute { time_samples, buffer })
}

/*// check that the topology is actually heterogeneous
let face_counts = mesh.face_counts.get(0).unwrap().values;
for i in 1..mesh.face_counts.sample_count() {
    let other_face_counts = mesh.face_counts.get(i).unwrap().values;
    if face_counts != other_face_counts {
        return Err(anyhow::anyhow!("unsupported topology variance"));
    }
}

let face_indices = mesh.face_indices.get(0).unwrap().values;
for i in 1..mesh.face_indices.sample_count() {
    let other_face_indices = mesh.face_indices.get(i).unwrap().values;
    if face_indices != other_face_indices {
        return Err(anyhow::anyhow!("unsupported topology variance"));
    }
}

eprintln!(
    "*** topology is declared as heterogeneous, but is actually homogeneous ({} {}) ****",
    mesh.face_counts.sample_count(),
    mesh.face_indices.sample_count()
);*/

impl Mesh3D {
    fn from_alembic(mesh: &PolyMesh) -> Result<Mesh3D, anyhow::Error> {
        let device = gpu::device();
        if !matches!(
            mesh.topology_variance(),
            MeshTopologyVariance::Constant | MeshTopologyVariance::Homogeneous
        ) {
            bail!("unsupported topology variance");
        }

        if mesh.face_counts.sample_count() == 0 || mesh.face_indices.sample_count() == 0 {
            bail!("empty mesh (no samples)");
        }

        // triangulate indices
        let face_counts = mesh.face_counts.get(0).unwrap().values;
        let indices = mesh.face_indices.get(0).unwrap().values;
        if face_counts.is_empty() || indices.is_empty() {
            bail!("empty mesh");
        }

        let index_count = indices.len();
        let triangle_count = face_counts.iter().map(|&count| (count - 2) as usize).sum::<usize>();
        let triangulated_index_count = triangle_count * 3;
        let mut index_buffer = device.create_array_buffer::<u32>(
            BufferUsage::INDEX_BUFFER,
            MemoryLocation::CpuToGpu,
            triangulated_index_count,
        );
        //eprintln!("loading mesh: faces: {}, indices: {}", face_counts.len(), indices.len());
        triangulate_indices(&face_counts, &indices, unsafe { index_buffer.as_mut_slice() });

        // load attribute samples (positions, etc.)

        // load attribute samples
        // SAFETY: we allocate enough space in attribute buffers to hold all samples
        let vertex_count = mesh.positions.dimensions(0)[0];
        let positions = read_attribute_samples(device, vertex_count, &mesh.positions)?;
        let normals = mesh
            .normals
            .as_ref()
            .map(|normals| read_geom_param(device, index_count, normals))
            .transpose()?;
        let uvs = mesh
            .uvs
            .as_ref()
            .map(|uvs| read_geom_param(device, index_count, uvs))
            .transpose()?;
        //let velocities = mesh.velocities.as_ref().map(|velocities| read_attribute_samples(device, "velocities", index_count, velocities)).transpose()?;

        // FIXME: normals and uvs are not triangulated!

        Ok(Mesh3D {
            index_count: triangulated_index_count as u32,
            positions,
            normals,
            uvs,
            indices: index_buffer,
        })
    }
}

impl Object {
    fn load_alembic_object_recursive(obj: &ObjectReader) -> Result<Object, anyhow::Error> {
        let mut transform = vec![];
        match XForm::new(obj.properties(), ".xform") {
            Ok(xform) => {
                for (time, sample) in xform.samples() {
                    transform.push(crate::scene::TimeSample {
                        time,
                        value: glam::DMat4::from_cols_array_2d(&sample),
                    });
                }
            }
            Err(err) => {}
        }

        let mesh = if let Ok(mesh) = PolyMesh::new(obj.properties(), ".geom") {
            match Mesh3D::from_alembic(&mesh) {
                Ok(mesh) => {
                    info!("loaded mesh: {}", obj.name());
                    Some(mesh)
                }
                Err(err) => {
                    warn!("failed to load mesh `{}`: {}", obj.path(), err);
                    None
                }
            }
        } else {
            None
        };

        let mut children = vec![];
        for child in obj.children() {
            children.push(Self::load_alembic_object_recursive(&child)?);
        }

        //eprintln!("loaded object: {}", obj.name());

        Ok(Object {
            name: obj.name().to_string(),
            transform,
            geometry: mesh,
            children,
        })
    }
}

impl Scene3D {
    fn load_from_alembic_inner(path: &Path) -> Result<Self, anyhow::Error> {
        let archive = alembic_ogawa::Archive::open(path)?;
        let root = Object::load_alembic_object_recursive(&archive.root()?)?;
        Ok(Self { root })
    }

    pub fn load_from_alembic(path: impl AsRef<Path>) -> Result<Self, anyhow::Error> {
        Self::load_from_alembic_inner(path.as_ref())
    }
}
