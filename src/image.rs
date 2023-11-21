use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use ahash::HashMap;
use eyre::{Result, WrapErr};

use crate::{
    logseq::db::MetadataDb,
    pic_store::{GetImageResult, PicStoreClient, PicStoreImageData},
};

pub struct Image {
    pub path: PathBuf,
    pub hash: blake3::Hash,
    pub data: Vec<u8>,
}

pub struct ImageInfo {
    pub image: Image,
    pub data: PicStoreImageData,
}

pub struct Images {
    images: std::sync::Mutex<HashMap<String, ImageInfo>>,
    pending_images: std::sync::Mutex<Vec<(Image, String)>>,
    base_path: PathBuf,
    pic_store: PicStoreClient,
    db: MetadataDb,
}

impl Images {
    pub fn new(base_path: PathBuf, pic_store: PicStoreClient, db: MetadataDb) -> Self {
        Self {
            images: Mutex::new(HashMap::default()),
            pending_images: Mutex::new(Vec::new()),
            base_path,
            pic_store,
            db,
        }
    }

    /// Read an image and upload it to the CDN if necessary.
    pub fn add(&self, path: PathBuf, upload_profile: Option<&str>) -> Result<()> {
        let full_path = self.base_path.join(&path);
        let image_data =
            std::fs::read(&full_path).wrap_err_with(|| format!("{}", full_path.display()))?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(&image_data);
        let hash = hasher.finalize();

        let rel_path = full_path
            .strip_prefix(&self.base_path)
            .unwrap_or(&full_path);

        let image = Image {
            path: PathBuf::from(rel_path),
            hash,
            data: image_data,
        };

        let db_entry = self.db.get_image(&image)?;

        if let Some(data) = db_entry {
            // We already have the image, so there's nothing to do.
            let mut images = self.images.lock().unwrap();
            images.insert(
                image.path.to_string_lossy().to_string(),
                ImageInfo { image, data },
            );
        } else {
            // This is a new image, so add it to the CDN if necessary.
            let result = self.pic_store.get_or_upload_image(&image, upload_profile)?;
            match result {
                GetImageResult::Exists(result) => {
                    self.db.add_image(&image, &result)?;
                    let mut images = self.images.lock().unwrap();
                    images.insert(
                        image.path.to_string_lossy().to_string(),
                        ImageInfo {
                            image,
                            data: result.combine_2x(),
                        },
                    );
                }
                GetImageResult::Uploaded(id) => {
                    let mut pending = self.pending_images.lock().unwrap();
                    pending.push((image, id));
                }
            }
        }

        Ok(())
    }

    /// Extract the image list once everything has been gathered.
    pub fn finish(self) -> Result<HashMap<String, ImageInfo>> {
        let pending = self.pending_images.into_inner().unwrap();
        let mut images = self.images.into_inner().unwrap();

        // For any images that we uploaded, wait for them to finish processing before we proceed.
        for (image, id) in pending {
            let path = image.path.to_string_lossy().to_string();
            loop {
                if let Some(info) = self.pic_store.get_image_status(&id)? {
                    self.db.add_image(&image, &info)?;
                    images.insert(path, ImageInfo { image, data: info });
                    break;
                }

                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }

        Ok(images)
    }
}

pub fn image_full_path(base_path: &Path, origin_path: &Path, image_path: &str) -> Option<PathBuf> {
    if image_path.starts_with("http") {
        return None;
    }

    origin_path
        .parent()
        .map(|p| p.join(image_path))
        .unwrap_or_else(|| PathBuf::from(image_path))
        .canonicalize()
        .ok()
        .map(|p| {
            p.strip_prefix(base_path)
                .map(|p| p.to_path_buf())
                .unwrap_or(p)
        })
}

pub const DEFAULT_PICTURE_TEMPLATE: &str = r##"
<picture>
{{#each output}}
  <source srcset="{{this.srcset}}" type="image/{{this.format}}" width="{{this.width}}" height="{{this.height}}" />
{{/each}}
  <img src="{{fallback.url}}" alt="{{alt}}" width="{{fallback.width}}" height="{{fallback.height}}" />
</picture>
"##;
