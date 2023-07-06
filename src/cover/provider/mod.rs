use crate::{config::CONFIG, context::Context};

use self::{imgbb::ImgBBProvider, musicbrainz::MusicBrainzProvider};

pub mod imgbb;
pub mod musicbrainz;

#[derive(Debug)]
pub enum Provider {
    ImgBB {
        client: ImgBBProvider,
    },
    MusicBrainz {
        client: musicbrainz::MusicBrainzProvider,
    },
    Unknown,
}

impl Provider {
    pub fn new<T>(name: T) -> Self
    where
        T: AsRef<str>,
    {
        let name = name.as_ref().to_lowercase();

        match name.as_str() {
            "imgbb" => match CONFIG.image.provider.imgbb.api_key.as_ref() {
                Some(api_key) => Self::ImgBB {
                    client: ImgBBProvider::new(api_key),
                },
                None => Self::Unknown,
            },
            "musicbrainz" => Self::MusicBrainz {
                client: MusicBrainzProvider::new(),
            },
            _ => Self::Unknown,
        }
    }

    pub async fn get_cover_url(&self, context: &Context) -> Option<String> {
        match self {
            Self::ImgBB { client } => client.get_cover_url(context).await,
            Self::MusicBrainz { client } => client.get_cover_url(context).await,
            Self::Unknown => None,
        }
    }
}
