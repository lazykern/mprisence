use std::path::Path;

use lofty::TaggedFileExt;
use mpris::Metadata;
use url::Url;
use walkdir::WalkDir;

use crate::{consts::DEFAULT_IMAGE_FILE_NAMES, CONFIG};

use self::{cache::Cache, provider::Provider};

pub mod cache;
pub mod provider;

lazy_static::lazy_static! {
    pub static ref COVER_URL_FINDER: CoverURLFinder = CoverURLFinder::new();
}

pub struct CoverURLFinder {
    cache: Cache,
    provider: Option<Provider>,
}

impl CoverURLFinder {
    pub fn new() -> Self {
        let provider = match CONFIG.image.provider.provider.to_lowercase().as_str() {
            "imgbb" => Some(Provider::new_imgbb(
                &CONFIG
                    .image
                    .provider
                    .imgbb
                    .api_key
                    .clone()
                    .unwrap_or_default(),
            )),
            _ => None,
        };
        CoverURLFinder {
            cache: Cache::new(),
            provider,
        }
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
        let bytes_hash = sha256::digest(bytes.as_ref());

        if let Some(url) = self.cache.get_image_url(&bytes_hash) {
            return Some(url);
        }

        if let Some(provider) = &self.provider {
            if let Ok(url) = provider.upload_bytes(bytes).await {
                self.cache.set_image_url(&bytes_hash, &url);
                return Some(url);
            }
        }

        None
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
