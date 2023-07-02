use std::path::Path;

use lofty::TaggedFileExt;
use mpris::Metadata;
use url::Url;
use walkdir::WalkDir;

use crate::{consts::DEFAULT_IMAGE_FILE_NAMES, CONFIG};

use self::{cache::Cache, provider::Provider};

pub mod cache;
pub mod provider;

pub struct ImageURLFinder {
    cache: Cache,
    provider: Option<Provider>,
}

impl ImageURLFinder {
    pub fn new(provider: Option<Provider>) -> Self {
        ImageURLFinder {
            cache: Cache::new(),
            provider,
        }
    }

    pub async fn from_metadata(&mut self, metadata: &Metadata) -> Option<String> {
        if let Some(meta_art_url) = metadata.art_url() {
            if let Ok(parsed_url) = Url::parse(meta_art_url) {
                if parsed_url.scheme() == "http" || parsed_url.scheme() == "https" {
                    return Some(meta_art_url.to_string());
                }
            }
        }

        let parsed_url = match metadata.url() {
            Some(url) => match Url::parse(url) {
                Ok(url) => url,
                Err(_) => {
                    return None;
                }
            },
            None => return None,
        };

        if parsed_url.scheme() == "http" || parsed_url.scheme() == "https" {
            return None;
        }

        let file_path = match parsed_url.to_file_path() {
            Ok(file_path) => file_path,
            Err(_) => return None,
        };

        self.from_path(file_path).await
    }

    pub async fn from_path<P>(&mut self, path: P) -> Option<String>
    where
        P: AsRef<Path>,
    {
        let bytes = match find_picture(&path) {
            Some(picture) => picture,
            None => return None,
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
        Err(_) => {
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
