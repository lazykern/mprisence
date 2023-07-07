pub mod cache;
pub mod provider;

pub use provider::Provider;

use crate::config::CONFIG;

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

lazy_static::lazy_static! {
    pub static ref PROVIDER: Provider = Provider::new(CONFIG.cover.provider.provider.as_str());
    pub static ref REQWEST_CLIENT: reqwest::Client = {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(APP_USER_AGENT)
        );
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json")
        );
        reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap()
    };
}
