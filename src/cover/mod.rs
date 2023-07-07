pub mod cache;
pub mod provider;

pub use provider::Provider;

use crate::config::{CONFIG, StringOrStringVec};

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

lazy_static::lazy_static! {
    pub static ref PROVIDERS: Vec<Provider> = {
        let mut providers = Vec::new();
        match CONFIG.cover.provider.provider {
            StringOrStringVec::String(ref provider) => {
                    providers.push(Provider::from_name(provider));
            }
            StringOrStringVec::Vec(ref _providers) => {
                for provider in _providers {
                    providers.push(Provider::from_name(provider));
                }
            }
        }
        providers
    };
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
