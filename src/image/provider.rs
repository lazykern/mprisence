use crate::Error;

pub enum Provider {
    ImgBB(imgbb::ImgBB),
}

impl Provider {
    pub fn new_imgbb<T>(api_key: T) -> Self
    where
        T: Into<String>,
    {
        Provider::ImgBB(imgbb::ImgBB::new(api_key))
    }

    pub async fn upload_bytes<T>(&self, bytes: T) -> Result<String, Error>
    where
        T: AsRef<[u8]>,
    {
        let art_url: String;

        match self {
            Provider::ImgBB(provider) => {
                let res = provider.upload_bytes(bytes).await?;
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

                art_url = match thumb.url {
                    Some(url) => url,
                    None => {
                        return Err(Error::ProviderError(
                            "No url field returned from ImgBB".to_string(),
                        ))
                    }
                };
            }
        }

        Ok(art_url)
    }

    pub async fn upload_file<T>(&self, path: T) -> Result<String, Error>
    where
        T: AsRef<std::path::Path>,
    {
        let art_url: String;

        match self {
            Provider::ImgBB(provider) => {
                let res = provider.upload_file(path).await?;
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

                art_url = match thumb.url {
                    Some(url) => url,
                    None => {
                        return Err(Error::ProviderError(
                            "No url field returned from ImgBB".to_string(),
                        ))
                    }
                };
            }
        }

        Ok(art_url)
    }

    pub async fn upload_base64<T>(&self, base64: T) -> Result<String, Error>
    where
        T: AsRef<str>,
    {
        let art_url: String;

        match self {
            Provider::ImgBB(provider) => {
                let res = provider.upload_base64(base64).await?;
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

                art_url = match thumb.url {
                    Some(url) => url,
                    None => {
                        return Err(Error::ProviderError(
                            "No url field returned from ImgBB".to_string(),
                        ))
                    }
                };
            }
        }

        Ok(art_url)
    }
}
