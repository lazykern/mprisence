use std::path::Path;

use imgbb::ImgBB;
use lofty::TaggedFileExt;
use mpris::Metadata;
use url::Url;
use walkdir::WalkDir;

use crate::{
    config::CONFIG, consts::DEFAULT_IMAGE_FILE_NAMES, context::Context, cover::cache::Cache, Error,
};

#[derive(Debug)]
pub struct ImgBBProvider {
    client: imgbb::ImgBB,
    cache: Cache,
}

impl ImgBBProvider {
    pub fn new<T>(api_key: T) -> Self
    where
        T: Into<String>,
    {
        Self {
            client: ImgBB::new(api_key),
            cache: Cache::new("imgbb"),
        }
    }

    pub async fn get_cover_url(&self, context: &Context) -> Option<String> {
        let cover_url = match context.metadata() {
            Some(metadata) => self.from_metadata(metadata).await,
            None => None,
        };

        if cover_url.is_none() {
            if let Some(path) = context.path() {
                return self.from_audio_path(path).await;
            }
        }

        cover_url
    }

    pub async fn from_metadata(&self, metadata: &Metadata) -> Option<String> {
        let meta_art_path = match metadata.art_url() {
            Some(meta_art_url) => {
                if let Ok(parsed_url) = Url::parse(meta_art_url) {
                    match parsed_url.scheme() {
                        "http" | "https" => {
                            return Some(meta_art_url.to_string());
                        }
                        "file" => match parsed_url.to_file_path() {
                            Ok(file_path) => Some(file_path),
                            Err(e) => {
                                log::error!("Failed to parse URL: {:?}", e);
                                None
                            }
                        },
                        _ => None,
                    }
                } else {
                    None
                }
            }
            _ => None,
        };

        let parsed_url = match metadata.url() {
            Some(url) => match Url::parse(url) {
                Ok(url) => url,
                Err(e) => {
                    log::error!("Failed to parse URL: {}", e);
                    return None;
                }
            },
            None => return None,
        };

        let mut cover_url = None;

        if parsed_url.scheme() == "file" {
            let file_path = match parsed_url.to_file_path() {
                Ok(file_path) => file_path,
                Err(_) => {
                    log::error!("Failed to get file path from URL",);
                    return None;
                }
            };
            cover_url = self.from_audio_path(file_path).await;
        }

        if cover_url.is_none() {
            match meta_art_path {
                Some(meta_art_path) => {
                    cover_url = self.from_image_path(meta_art_path).await;
                }
                None => {}
            }
        }

        cover_url
    }

    pub async fn from_audio_path<P>(&self, path: P) -> Option<String>
    where
        P: AsRef<Path>,
    {
        let bytes = match find_picture(&path) {
            Some(picture) => picture,
            None => return None,
        };

        self.from_bytes(bytes).await
    }

    pub async fn from_image_path<P>(&self, path: P) -> Option<String>
    where
        P: AsRef<Path>,
    {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                log::error!("Failed to read image file: {}", e);
                return None;
            }
        };

        self.from_bytes(bytes).await
    }

    pub async fn from_bytes<T>(&self, bytes: T) -> Option<String>
    where
        T: AsRef<[u8]>,
    {
        let bytes = bytes.as_ref();

        let cache_key = sha256::digest(bytes);

        if let Some(url) = self.cache.get(&cache_key) {
            return Some(url);
        }

        if let Ok(url) = self.upload_bytes(bytes).await {
            self.cache.set(&cache_key, &url);
            return Some(url);
        }

        None
    }

    pub async fn upload_bytes<T>(&self, bytes: T) -> Result<String, Error>
    where
        T: AsRef<[u8]>,
    {
        let cover_url: String;

        let res = self.client.upload_bytes(bytes).await?;
        let data = match res.data {
            Some(data) => data,
            None => {
                return Err(Error::ProviderError(
                    "No data field returned from ImgBB".to_string(),
                ))
            }
        };

        let thumb = match data.thumb {
            Some(thumb) => thumb,
            None => {
                return Err(Error::ProviderError(
                    "No thumb field returned from ImgBB".to_string(),
                ))
            }
        };

        cover_url = match thumb.url {
            Some(url) => url,
            None => {
                return Err(Error::ProviderError(
                    "No url field returned from ImgBB".to_string(),
                ))
            }
        };

        Ok(cover_url)
    }

    pub async fn upload_file<T>(&self, path: T) -> Result<String, Error>
    where
        T: AsRef<std::path::Path>,
    {
        let cover_url: String;

        let res = self.client.upload_file(path).await?;
        let data = match res.data {
            Some(data) => data,
            None => {
                return Err(Error::ProviderError(
                    "No data field returned from ImgBB".to_string(),
                ))
            }
        };

        let thumb = match data.thumb {
            Some(thumb) => thumb,
            None => {
                return Err(Error::ProviderError(
                    "No thumb field returned from ImgBB".to_string(),
                ))
            }
        };

        cover_url = match thumb.url {
            Some(url) => url,
            None => {
                return Err(Error::ProviderError(
                    "No url field returned from ImgBB".to_string(),
                ))
            }
        };

        Ok(cover_url)
    }

    pub async fn upload_base64<T>(&self, base64: T) -> Result<String, Error>
    where
        T: AsRef<str>,
    {
        let cover_url: String;

        let res = self.client.upload_base64(base64).await?;
        let data = match res.data {
            Some(data) => data,
            None => {
                return Err(Error::ProviderError(
                    "No data field returned from ImgBB".to_string(),
                ))
            }
        };

        let thumb = match data.thumb {
            Some(thumb) => thumb,
            None => {
                return Err(Error::ProviderError(
                    "No thumb field returned from ImgBB".to_string(),
                ))
            }
        };

        cover_url = match thumb.url {
            Some(url) => url,
            None => {
                return Err(Error::ProviderError(
                    "No url field returned from ImgBB".to_string(),
                ))
            }
        };

        Ok(cover_url)
    }
}

fn find_picture<P>(path: P) -> Option<Vec<u8>>
where
    P: AsRef<Path>,
{
    match get_first_embedded_picture(&path) {
        Some(picture) => Some(picture),

        None => {
            let mut file_names: Vec<String> = DEFAULT_IMAGE_FILE_NAMES
                .iter()
                .map(|s| s.to_string())
                .collect();
            for name in &CONFIG.image.file_names {
                if !file_names.contains(&name) {
                    file_names.push(name.to_string());
                }
            }

            get_picture_from_base_path(&path, &file_names)
        }
    }
}

fn get_first_embedded_picture<P>(path: P) -> Option<Vec<u8>>
where
    P: AsRef<Path>,
{
    let parsed_file = match lofty::read_from_path(path) {
        Ok(parsed_file) => parsed_file,
        Err(e) => {
            log::error!("Failed to parse file: {}", e);
            return None;
        }
    };

    let tagged_file = match parsed_file.primary_tag() {
        Some(tagged_file) => tagged_file,
        None => match parsed_file.first_tag() {
            Some(tagged_file) => tagged_file,
            None => {
                return None;
            }
        },
    };

    let pictures = tagged_file.pictures();

    pictures.first().map(|p| p.data().to_vec())
}

fn get_picture_from_base_path<P>(path: P, picture_file_names: &Vec<String>) -> Option<Vec<u8>>
where
    P: AsRef<Path>,
{
    let p = path.as_ref();

    let base_path = match p.is_file() {
        true => p.parent().unwrap(),
        false => p,
    };

    let walker = WalkDir::new(base_path);

    for e in walker.max_depth(1).follow_links(true).into_iter() {
        let entry = match e {
            Ok(entry) => entry,
            Err(_) => {
                continue;
            }
        };

        if entry.path().is_dir() {
            continue;
        }

        for picture_file_name in picture_file_names {
            if entry
                .file_name()
                .to_str()
                .unwrap_or("")
                .split(".")
                .next()
                .unwrap_or("")
                == *picture_file_name
            {
                return match std::fs::read(entry.path()) {
                    Ok(picture) => Some(picture),
                    Err(_) => None,
                };
            }
        }
    }

    None
}
