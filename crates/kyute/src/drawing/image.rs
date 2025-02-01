//! Wrapper around skia images.
use crate::drawing::ToSkia;
use crate::Size;
use skia_safe::image::CachingHint;
use std::io;
use std::ops::Deref;
use std::sync::OnceLock;

/// An image. Paper-thin wrapper around skia images.
#[derive(Clone, Debug)]
pub struct Image(skia_safe::Image);

impl Image {
    /// Returns the size in pixels of the image.
    pub fn size(&self) -> Size {
        let s = self.0.dimensions();
        Size::new(s.width as f64, s.height as f64)
    }

    /// Loads an image from the bytes of a file.
    pub fn load_from_file_bytes(bytes: &[u8]) -> Result<Self, io::Error> {
        unsafe {
            // There used to be a public `DecodeToRaster` API that could take a void* but it was removed because it was "unused"
            let data = skia_safe::Data::new_bytes(bytes);
            let image = skia_safe::Image::from_encoded(data)
                .ok_or(io::Error::new(io::ErrorKind::InvalidData, "failed to load image data"))?
                // must call to force decoding and release
                // TODO not sure about "Disallow"
                .make_raster_image(None, CachingHint::Disallow)
                .ok_or(io::Error::new(io::ErrorKind::InvalidData, "failed to decode image"))?;
            Ok(Image(image))
        }
    }
}

impl ToSkia for Image {
    type Target = skia_safe::Image;

    fn to_skia(&self) -> Self::Target {
        self.0.clone()
    }
}

/// A static image embedded in the binary.
///
/// # Note
///
/// The image is decoded lazily on first access via the `Deref` implementation.
pub struct StaticImage {
    path: Option<&'static str>,
    data: &'static [u8],
    image: OnceLock<Image>,
}

impl StaticImage {
    /// Creates a new static image from the contents of a file.
    ///
    /// # Example
    ///
    /// ```rust
    /// use kyute::drawing::StaticImage;
    /// pub const MY_IMAGE: StaticImage = StaticImage::new(include_bytes!("my_image.png"));
    /// ```
    pub const fn new(file_data: &'static [u8]) -> StaticImage {
        StaticImage::new_with_path(file_data, None)
    }

    const fn new_with_path(data: &'static [u8], path: Option<&'static str>) -> StaticImage {
        StaticImage {
            path,
            data,
            image: OnceLock::new(),
        }
    }

    fn load(&self) -> &Image {
        self.image.get_or_init(|| match Image::load_from_file_bytes(self.data) {
            Ok(image) => image,
            Err(err) => {
                if let Some(path) = self.path {
                    panic!("failed to load static image (from {}): {}", path, err);
                } else {
                    panic!("failed to load static image: {}", err);
                }
            }
        })
    }
}

impl Deref for StaticImage {
    type Target = Image;

    fn deref(&self) -> &Self::Target {
        self.load()
    }
}

/// Macro to embed a static image from a file.
///
/// This uses `include_bytes!` to embed the file in the binary.
/// This produces a constant of type `StaticImage`.
#[macro_export]
macro_rules! static_image {
    ($path:literal) => {
        $crate::drawing::image::StaticImage::new(include_bytes!($path), Some($path))
    };
}
