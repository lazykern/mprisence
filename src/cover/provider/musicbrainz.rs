use crate::context::Context;

#[derive(Debug)]
pub struct MusicBrainzProvider {}

impl MusicBrainzProvider {
    pub async fn get_cover_url(&self, _context: &Context) -> Option<String> {
        unimplemented!();
    }
}
