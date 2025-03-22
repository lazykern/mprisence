use async_trait::async_trait;
use mpris::Metadata;

use super::error::CoverArtError;

pub mod imgbb;
pub mod musicbrainz;

#[async_trait]
pub trait CoverArtProvider {
    fn name(&self) -> &'static str;
    async fn get_cover_url(&self, metadata: &Metadata) -> Result<Option<String>, CoverArtError>;
}
