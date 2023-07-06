pub mod cache;
pub mod provider;

pub use provider::Provider;

use crate::config::CONFIG;

lazy_static::lazy_static! {
    pub static ref PROVIDER: Provider = Provider::new(CONFIG.image.provider.provider.as_str());
}
