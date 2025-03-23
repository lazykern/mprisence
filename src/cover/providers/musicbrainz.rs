use async_trait::async_trait;
use log::debug;
use mpris::Metadata;
use musicbrainz_rs::{
    entity::{
        recording::{Recording, RecordingSearchQuery},
        release::{Release, ReleaseSearchQuery},
        release_group::{ReleaseGroup, ReleaseGroupSearchQuery},
        CoverartResponse,
    },
    FetchCoverart, Search,
};

use crate::cover::error::CoverArtError;

use super::CoverArtProvider;

#[derive(Clone)]
pub struct MusicbrainzProvider;

impl MusicbrainzProvider {
    pub fn new() -> Self {
        debug!("Creating MusicbrainzProvider");
        Self {}
    }

    // Unified search function that handles both release groups and releases
    async fn search_album<S: AsRef<str>>(
        &self,
        album: S,
        artists: &[S],
    ) -> Result<Option<String>, CoverArtError> {
        // First try release groups
        let mut builder = ReleaseGroupSearchQuery::query_builder();
        builder.release_group(album.as_ref());
        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
        }

        let results = ReleaseGroup::search(builder.build()).execute().await?;

        for group in results.entities.iter().take(2) {
            let cover = group.get_coverart().front().res_250().execute().await?;
            if let CoverartResponse::Url(url) = cover {
                return Ok(Some(url));
            }

            // Try covers from releases in the group
            if let Some(releases) = &group.releases {
                for release in releases.iter().take(2) {
                    let cover = release.get_coverart().front().res_250().execute().await?;
                    if let CoverartResponse::Url(url) = cover {
                        return Ok(Some(url));
                    }
                }
            }
        }

        // If no release group covers found, try direct release search
        let mut builder = ReleaseSearchQuery::query_builder();
        builder.release(album.as_ref());
        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
        }

        let results = Release::search(builder.build()).execute().await?;

        for release in results.entities.iter().take(2) {
            let cover = release.get_coverart().front().res_250().execute().await?;
            if let CoverartResponse::Url(url) = cover {
                return Ok(Some(url));
            }
        }

        Ok(None)
    }

    // Search by track/recording
    async fn search_track<S: AsRef<str>>(
        &self,
        track: S,
        artists: &[S],
        duration_ms: Option<u128>,
    ) -> Result<Option<String>, CoverArtError> {
        let mut builder = RecordingSearchQuery::query_builder();
        builder.recording(track.as_ref());
        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
        }

        if let Some(duration) = duration_ms {
            builder
                .and()
                .duration(format!("[{} TO {}]", duration - 3000, duration + 3000).as_str());
        }

        let results = Recording::search(builder.build())
            .with_releases()
            .execute()
            .await?;

        for recording in results.entities.iter().take(3) {
            if let Some(releases) = &recording.releases {
                for release in releases.iter().take(2) {
                    // Try release cover
                    let cover = release.get_coverart().front().res_250().execute().await?;
                    if let CoverartResponse::Url(url) = cover {
                        return Ok(Some(url));
                    }

                    // Try release group cover
                    if let Some(rg) = &release.release_group {
                        let cover = rg.get_coverart().front().res_250().execute().await?;
                        if let CoverartResponse::Url(url) = cover {
                            return Ok(Some(url));
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}

#[async_trait]
impl CoverArtProvider for MusicbrainzProvider {
    fn name(&self) -> &'static str {
        "musicbrainz"
    }

    async fn get_cover_url(&self, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        let artists = metadata.artists().unwrap_or_default();
        let artists = artists.as_slice();
        let album_artists = metadata.album_artists().unwrap_or_default();
        let album_artists = album_artists.as_slice();

        // Try album search first if we have album metadata
        if let Some(album) = metadata.album_name() {
            let search_artists = if !album_artists.is_empty() {
                album_artists
            } else {
                artists
            };

            if !search_artists.is_empty() {
                if let Some(url) = self.search_album(album, search_artists).await? {
                    return Ok(Some(url));
                }
            }
        }

        // Fall back to track search
        if let Some(title) = metadata.title() {
            if !artists.is_empty() {
                let duration = metadata.length().map(|d| d.as_millis());
                if let Some(url) = self.search_track(title, artists, duration).await? {
                    return Ok(Some(url));
                }
            }
        }

        Ok(None)
    }
}
