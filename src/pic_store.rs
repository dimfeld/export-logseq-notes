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
                // unwrap is safe because the config parsing stage ensures that the key is set.
                format!("Bearer {}", config.api_key.as_ref().unwrap()).try_into()?,
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
                let mut image_data: PicStoreImageData = lookup_response.json()?;
                image_data
                    .output
                    .sort_unstable_by_key(|o| o.file_size.unwrap_or_default());
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
    pub fn get_or_upload_image(
        &self,
        image: &Image,
        upload_profile: Option<&str>,
    ) -> Result<GetImageResult> {
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
                .as_ref()
                .map(|prefix| format!("{}/{}", prefix, filename)),
            upload_profile_id: upload_profile
                .map(|p| p.to_string())
                .or_else(|| self.config.upload_profile.clone()),
        };

        println!("Uploading {}...", filename);
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

    /// Get the status of an image, returning it only if the image is ready to read.
    pub fn get_image_status(&self, image_id: &str) -> Result<Option<PicStoreImageData>> {
        let url = format!("{}/api/images/{}", self.config.url, image_id);
        let mut response: PicStoreImageData =
            self.client.get(&url).send()?.error_for_status()?.json()?;

        if response.status == "ready" {
            response
                .output
                .sort_unstable_by_key(|o| o.file_size.unwrap_or_default());
            Ok(Some(response))
        } else {
            Ok(None)
        }
    }
}

pub enum GetImageResult {
    Uploaded(String),
    Exists(PicStoreImageData),
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewBaseImageResult {
    pub id: String,
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
    pub status: String,
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub alt_text: String,
    pub file_size: Option<u32>,
    pub output: Vec<PicStoreImageOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PicStoreImageOutput {
    pub id: String,
    pub url: String,
    pub srcset: Option<String>,
    pub status: String,
    pub file_size: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub format: String,
}

impl PicStoreImageData {
    pub fn combine_2x(mut self) -> Self {
        let double_size_indexes = self
            .output
            .iter()
            .map(|image| {
                if image.srcset.is_some() {
                    return String::new();
                }

                if let Some(width) = image.width {
                    let double_width = self
                        .output
                        .iter()
                        .find(|i| i.width.unwrap_or(0) == width * 2);
                    if let Some(dw) = double_width {
                        return dw.url.clone();
                    }
                }

                if let Some(height) = image.height {
                    let double_height = self
                        .output
                        .iter()
                        .find(|i| i.height.unwrap_or(0) == height * 2);
                    if let Some(dh) = double_height {
                        return dh.url.clone();
                    }
                }

                return String::new();
            })
            .collect::<Vec<_>>();

        for (image, double_url) in self.output.iter_mut().zip(double_size_indexes.into_iter()) {
            if double_url.is_empty() {
                image.srcset = Some(image.url.clone());
            } else {
                image.srcset = Some(format!("{}, {} 2x", image.url, double_url));
            }
        }

        self
    }
}
