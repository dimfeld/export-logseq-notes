use std::{path::PathBuf, sync::Mutex};

use ahash::HashMap;
use eyre::Result;

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
    pub fn add(&self, path: PathBuf) -> Result<()> {
        let image_data = std::fs::read(self.base_path.join(&path))?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(&image_data);
        let hash = hasher.finalize();

        let image = Image {
            path,
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
            let result = self.pic_store.get_or_upload_image(&image)?;
            match result {
                GetImageResult::Exists(result) => self.db.add_image(&image, &result)?,
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
            }
        }

        Ok(images)
    }
}
