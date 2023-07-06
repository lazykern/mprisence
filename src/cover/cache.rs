use dirs::cache_dir;
use sha256::digest;
use std::{fs, path::PathBuf};

use crate::consts::APP_NAME;

#[derive(Debug)]
pub enum Cache {
    Ok(PathBuf),
    Err,
}

impl Cache {
    pub fn new<T>(provider: T) -> Self
    where
        T: AsRef<str>,
    {
        if let Some(cache_dir) = cache_dir() {
            let cache_dir = cache_dir.join(APP_NAME).join(provider.as_ref());
            if !cache_dir.exists() {
                fs::create_dir_all(&cache_dir).unwrap_or_default();
            }

            Self::Ok(cache_dir)
        } else {
            log::error!("Failed to get cache directory");
            Self::Err
        }
    }

    pub fn get_image_url<T>(&self, key: T) -> Option<String>
    where
        T: AsRef<str>,
    {
        let cache_dir_path = match self {
            Self::Ok(cache_dir_path) => cache_dir_path,
            Self::Err => return None,
        };
        let key = key.as_ref();
        let file_path = cache_dir_path.join(digest(key));

        if file_path.exists() {
            match fs::read_to_string(file_path) {
                Ok(url) => Some(url),
                Err(e) => {
                    log::error!("Failed to read cache file: {}", e);
                    None
                }
            }
        } else {
            None
        }
    }

    pub fn set_image_url<T>(&self, key: T, url: T)
    where
        T: AsRef<str>,
    {
        let cache_dir = match self {
            Self::Ok(cache_dir) => cache_dir,
            Self::Err => return,
        };

        let key = key.as_ref();
        let url = url.as_ref();

        let file_path = cache_dir.join(digest(key));

        fs::write(file_path, url).unwrap_or_default();
    }
}
