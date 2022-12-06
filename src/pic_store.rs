use eyre::Result;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::{config::PicStoreConfig, image::Image};

pub struct PicStoreClient {
    client: reqwest::blocking::Client,
    config: PicStoreConfig,
}

impl PicStoreClient {
    pub fn new(config: &PicStoreConfig) -> eyre::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("export-logseq-notes")
            .default_headers(HeaderMap::from_iter([(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", config.api_key).try_into()?,
            )]))
            .build()?;

        Ok(Self {
            client,
            config: config.clone(),
        })
    }

    fn lookup_by_hash(&self, hash: &blake3::Hash) -> Result<Option<PicStoreImageData>> {
        let url = format!("{}/api/image_by_hash/{}", self.config.url, hash);
        let lookup_response = self.client.get(&url).send()?;

        match lookup_response.status() {
            reqwest::StatusCode::OK => {
                let image_data: PicStoreImageData = lookup_response.json()?;
                Ok(Some(image_data))
            }
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            status => {
                let data: serde_json::Value = lookup_response.json()?;
                Err(eyre::eyre!(
                    "Unexpected response {} from Pic Store: {:?}",
                    status,
                    data
                ))
            }
        }
    }

    /// Get the image data if it exists. If it does not exist, upload it and return the ID,
    /// which can be checked again.
    pub fn get_or_upload_image(&self, image: &Image) -> Result<GetImageResult> {
        let existing = self.lookup_by_hash(&image.hash)?;

        if let Some(existing) = existing {
            return Ok(GetImageResult::Exists(existing));
        }

        let filename = image.path.file_name().unwrap().to_string_lossy();
        let new_image_spec = NewBaseImageRequest {
            filename: filename.to_string(),
            location: self
                .config
                .location_prefix
                .map(|prefix| format!("{}/{}", prefix, filename)),
            upload_profile_id: self.config.upload_profile,
        };

        let new_image: NewBaseImageResult = self
            .client
            .post(&format!("{}/api/images", self.config.url))
            .json(&new_image_spec)
            .send()?
            .error_for_status()?
            .json()?;

        let upload_url = format!("{}/api/images/{}/upload", self.config.url, new_image.id);
        self.client
            .post(&upload_url)
            .body(image.data.clone())
            .send()?
            .error_for_status()?;

        Ok(GetImageResult::Uploaded(new_image.id))
    }

    pub fn get_image_status(&self, image_id: &str) -> Result<Option<PicStoreImageData>> {
        todo!()
    }
}

pub enum GetImageResult {
    Uploaded(String),
    Exists(PicStoreImageData),
}

#[derive(Debug, Clone, Deserialize)]
struct NewBaseImageResult {
    id: String,
}

#[derive(Debug, Clone, Serialize)]
struct NewBaseImageRequest {
    filename: String,
    location: Option<String>,
    upload_profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PicStoreImageData {
    pub id: String,
    pub html: String,
    pub status: String,
    pub output: Vec<PicStoreImageOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PicStoreImageOutput {
    id: String,
    url: String,
    status: String,
    width: u32,
    height: u32,
    format: String,
}
