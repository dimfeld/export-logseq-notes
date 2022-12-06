use std::{path::PathBuf, sync::Mutex};

use ahash::HashMap;
use eyre::Result;

use crate::{logseq::db::MetadataDb, pic_store::PicStoreClient};

pub struct Image {
    pub path: PathBuf,
    pub hash: blake3::Hash,
    pub data: Vec<u8>,
}

pub struct ImageInfo {
    image: Image,
    html: String,
}

pub struct Images {
    images: std::sync::Mutex<HashMap<String, ImageInfo>>,
    pic_store: PicStoreClient,
    db: MetadataDb,
}

impl Images {
    pub fn new(pic_store: PicStoreClient, db: MetadataDb) -> Self {
        Self {
            images: Mutex::new(HashMap::default()),
            pic_store,
            db,
        }
    }

    /// Read an image and upload it to the CDN if necessary.
    pub fn add(&self, path: PathBuf) -> Result<()> {
        let image_data = std::fs::read(&path)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(&image_data);
        let hash = hasher.finalize();

        let image = Image {
            path,
            hash,
            data: image_data,
        };

        let db_entry = self.db.get_image(&image)?;

        if let Some(html) = db_entry {
            // We already have the image, so there's nothing to do.
            let mut images = self.images.lock().unwrap();
            images.insert(
                image.path.to_string_lossy().to_string(),
                ImageInfo { image, html },
            );
        } else {
            // This is a new image, so add it to the CDN if necessary.
            let result = self.pic_store.get_or_upload_image(&image)?;
            self.db.add_image(&image, &result.id, &result.html)?;
        }

        Ok(())
    }

    /// Extract the image list once everything has been gathered.
    pub fn finish(self) -> HashMap<String, ImageInfo> {
        self.images.into_inner().unwrap()
    }
}
