use std::path::Path;

use eyre::Result;
use reqwest::header::HeaderMap;
use serde::Deserialize;

use crate::config::PicStoreConfig;

pub struct PicStoreClient {
    client: reqwest::Client,
    config: PicStoreConfig,
}

impl PicStoreClient {
    pub fn new(config: &PicStoreConfig) -> eyre::Result<Self> {
        let client = reqwest::Client::builder()
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

    async fn lookup_by_hash(&self, image_data: &[u8]) -> Result<Option<PicStoreImageData>> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(image_data);
        let hash = hasher.finalize();

        let url = format!("{}/api/image_by_hash/{}", self.config.url, hash);
        let lookup_response = self.client.get(&url).send().await?;

        match lookup_response.status() {
            reqwest::StatusCode::OK => {
                let image_data: PicStoreImageData = lookup_response.json().await?;
                Ok(Some(image_data))
            }
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            status => {
                let data: serde_json::Value = lookup_response.json().await?;
                Err(eyre::eyre!(
                    "Unexpected response {} from Pic Store: {:?}",
                    status,
                    data
                ))
            }
        }
    }

    pub async fn get_or_upload_image(&self, image_path: &Path) -> Result<PicStoreImageData> {
        let image_data = std::fs::read(image_path)?;
        let existing = self.lookup_by_hash(&image_data).await?;

        if let Some(existing) = existing {
            return Ok(existing);
        }

        todo!("upload the image");
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PicStoreImageData {
    // todo
}
