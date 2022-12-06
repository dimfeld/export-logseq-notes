use eyre::Result;
use reqwest::header::HeaderMap;
use serde::Deserialize;

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

    pub fn get_or_upload_image(&self, image: &Image) -> Result<PicStoreImageData> {
        let existing = self.lookup_by_hash(&image.hash)?;

        if let Some(existing) = existing {
            return Ok(existing);
        }

        todo!("upload the image");
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PicStoreImageData {
    pub id: String,
    pub html: String,
}
