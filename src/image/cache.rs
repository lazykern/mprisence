use dirs::cache_dir;
use sha256::digest;
use std::fs;

use crate::consts::APP_NAME;

pub struct Cache {}

impl Cache {
    pub fn new() -> Self {
        if let Some(cache_dir) = cache_dir() {
            let cache_dir = cache_dir.join(APP_NAME);
            if !cache_dir.exists() {
                fs::create_dir_all(&cache_dir).unwrap_or_default();
            }
        }

        Cache {}
    }

    pub fn get_image_url<T>(&self, key: T) -> Option<String>
    where
        T: AsRef<str>,
    {
        let key = key.as_ref();
        let cache_dir = cache_dir()?.join(APP_NAME);
        let file_path = cache_dir.join(digest(key));

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
        let key = key.as_ref();
        let url = url.as_ref();
        let cache_dir = match cache_dir() {
            Some(cache_dir) => cache_dir.join(APP_NAME),
            None => return,
        };
        let file_path = cache_dir.join(digest(key));

        fs::write(file_path, url).unwrap_or_default();
    }
}
