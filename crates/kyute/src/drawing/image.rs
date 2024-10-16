//! Wrapper around skia images.

use crate::{drawing::ToSkia, Size};

/// An image. Paper-thin wrapper around skia images.
#[derive(Clone, Debug)]
pub struct Image(skia_safe::Image);

impl Image {
    /// Returns the size in pixels of the image.
    pub fn size(&self) -> Size {
        let s = self.0.dimensions();
        Size::new(s.width as f64, s.height as f64)
    }
}

impl ToSkia for Image {
    type Target = skia_safe::Image;

    fn to_skia(&self) -> Self::Target {
        self.0.clone()
    }
}

/*
impl Asset for Image {
    type LoadError = io::Error;

    fn load(reader: &mut dyn Read) -> Result<Self, Self::LoadError> {
        let mut data = vec![];
        reader.read_to_end(&mut data)?;
        Self::load_from_bytes(&data)
    }

    fn load_from_bytes(bytes: &[u8]) -> Result<Self, Self::LoadError> {
        unsafe {
            // There used to be a public `DecodeToRaster` API that could take a void* but it was removed because it was "unused"
            let sk_data = skia_safe::Data::new_bytes(bytes);
            let sk_image = skia_safe::Image::from_encoded(sk_data)
                .unwrap()
                .to_raster_image(None) // must call to force decoding and release
                .unwrap();
            Ok(Image(sk_image))
        }
    }
}*/

/*
/// Image cache entry.
#[derive(Clone)]
struct Entry {
    image: Image,
}

/// Image cache innards.
struct Inner {
    entries: HashMap<String, Entry>,
}*/

/*
/// Loads and caches images by URI.
#[derive(Clone)]
pub struct ImageCache {
    asset_loader: AssetLoader,
    inner: Arc<Mutex<Inner>>,
}

impl ImageCache {
    pub fn new(asset_loader: AssetLoader) -> ImageCache {
        ImageCache {
            asset_loader,
            inner: Arc::new(Mutex::new(Inner {
                entries: Default::default(),
            })),
        }
    }

    pub fn load(&self, uri: &str) -> Result<Image, AssetLoadError<io::Error>> {
        let mut inner = self.inner.lock().unwrap();

        if let Some(entry) = inner.entries.get(uri) {
            return Ok(entry.image.clone());
        }

        let image = self.asset_loader.load::<Image>(uri)?;
        inner.entries.insert(uri.to_owned(), Entry { image: image.clone() });
        Ok(image)
    }
}
*/

//impl_env_value!(ImageCache);

//pub const IMAGE_CACHE: EnvKey<ImageCache> = builtin_env_key!("kyute.image-cache");
