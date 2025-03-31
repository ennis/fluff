use bytemuck::cast_slice;
use glam::{dmat4, dvec4};
use graal::util::CommandStreamExt;
use graal::{CommandStream, Format, Image, ImageCreateInfo, ImageType, ImageUsage, MemoryLocation};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::{fs, io};

/// Given a path of the form `foo####.ext`, with `####` being a frame number, returns a list of all files in the same directory
/// that follow the same pattern, sorted by frame number.
///
/// # Limitations
///
/// This won't work if the stem (the part before the frame number) contains digits.
pub fn resolve_file_sequence(path: &Path) -> io::Result<Vec<(usize, PathBuf)>> {
    // TODO error handling for invalid paths
    let file_name = path.file_name().unwrap().to_str().unwrap();
    let parent_dir = path.parent().unwrap();

    let mut results = vec![];

    let re = Regex::new(r"(\D*)(\d+)\.(\w*)").unwrap();

    if let Some(c) = re.captures(&file_name) {
        let stem = c.get(1).unwrap().as_str();
        let ext = c.get(3).unwrap().as_str();
        //eprintln!("loading {stem}####.{ext}");
        let files_in_directory = fs::read_dir(parent_dir)?;
        for entry in files_in_directory {
            if let Ok(entry) = entry {
                if let Some(name) = entry.file_name().to_str() {
                    if let Some(c) = re.captures(&name) {
                        //eprintln!("candidate: {}", name);
                        let candidate_stem = c.get(1).unwrap().as_str();
                        let candidate_frame = c.get(2).unwrap().as_str().parse::<usize>().unwrap();
                        let candidate_ext = c.get(3).unwrap().as_str();

                        if candidate_stem == stem && candidate_ext == ext {
                            results.push((candidate_frame, entry.path()));
                        }
                    }
                }
            }
        }
        results.sort_by_key(|a| a.0);
        Ok(results)
    } else {
        Ok(vec![(0, path.to_path_buf())])
    }
}

/// Loads a 2D image file into a GPU texture.
pub fn load_rgba_texture(
    cmd: &mut CommandStream,
    path: impl AsRef<Path>,
    format: Format,
    usage: ImageUsage,
    mipmaps: bool,
) -> Image {
    let path = path.as_ref();
    //let device = cmd.device().clone();
    let image_io = image::open(path).expect("could not open image file");
    let width = image_io.width();
    let height = image_io.height();
    //let mip_levels = graal::mip_level_count(width, height);

    let vec_u8;
    let vec_u16;
    let vec_f32;
    let data: &[u8];

    match format {
        Format::R8_SRGB => {
            vec_u8 = image_io.to_luma8().to_vec();
            data = &vec_u8[..];
        }
        Format::R16_SNORM | Format::R16_SINT | Format::R16_UINT => {
            vec_u16 = image_io.to_luma16().to_vec();
            data = cast_slice(&vec_u16[..]);
        }
        Format::R8G8B8A8_SRGB
        | Format::R8G8B8A8_UNORM
        | Format::R8G8B8A8_SNORM
        | Format::R8G8B8A8_SINT
        | Format::R8G8B8A8_UINT => {
            vec_u8 = image_io.to_rgba8().to_vec();
            data = &vec_u8[..];
        }
        Format::R8G8B8_SRGB
        | Format::R8G8B8_UNORM
        | Format::R8G8B8_SNORM
        | Format::R8G8B8_UINT
        | Format::R8G8B8_SINT => {
            vec_u8 = image_io.to_rgb8().to_vec();
            data = &vec_u8[..];
        }
        Format::R32_SFLOAT => {
            vec_f32 = image_io.to_luma32f().to_vec();
            data = cast_slice(&vec_f32[..]);
        }
        Format::R32G32B32A32_SFLOAT => {
            vec_f32 = image_io.to_rgba32f().to_vec();
            data = cast_slice(&vec_f32[..]);
        }
        _ => panic!("unsupported format"),
    };

    cmd.create_image_with_data(
        &ImageCreateInfo {
            memory_location: MemoryLocation::GpuOnly,
            type_: ImageType::Image2D,
            usage: usage | ImageUsage::TRANSFER_DST,
            format,
            width,
            height,
            ..Default::default()
        },
        data,
    )
}

/// Returns the coefficients of the Lagrange interpolation polynomial for the given points.
pub fn lagrange_interpolate_4(p1: [f64; 2], p2: [f64; 2], p3: [f64; 2], p4: [f64; 2]) -> [f64; 4] {
    let ys = dvec4(p1[1], p2[1], p3[1], p4[1]);
    let xs = dvec4(p1[0], p2[0], p3[0], p4[0]);
    let v = dmat4(dvec4(1.0, 1.0, 1.0, 1.0), xs, xs * xs, xs * xs * xs);
    let v_inv = v.inverse();
    let coeffs = v_inv * ys;
    coeffs.to_array()
}
