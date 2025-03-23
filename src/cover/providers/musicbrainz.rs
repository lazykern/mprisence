use async_trait::async_trait;
use log::{debug, info};
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

use super::{CoverArtProvider, CoverResult};
use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;

#[derive(Clone)]
pub struct MusicbrainzProvider;

impl MusicbrainzProvider {
    pub fn new() -> Self {
        debug!("Creating new MusicBrainz provider");
        Self {}
    }

    async fn search_album<S: AsRef<str>>(
        &self,
        album: S,
        artists: &[S],
    ) -> Result<Option<String>, CoverArtError> {
        info!("Searching MusicBrainz for album: {}", album.as_ref());

        // Try release groups first
        let mut builder = ReleaseGroupSearchQuery::query_builder();
        builder.release_group(album.as_ref());

        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
            debug!("Including artist in search: {}", artist.as_ref());
        }

        let results = ReleaseGroup::search(builder.build()).execute().await?;

        if !results.entities.is_empty() {
            debug!("Found {} release group results", results.entities.len());

            for group in results.entities.iter().take(3) {
                // Try the release group cover
                let cover = group.get_coverart().front().res_500().execute().await?;
                if let CoverartResponse::Url(url) = cover {
                    info!("Found cover art URL from release group");
                    return Ok(Some(url));
                }

                // Try releases in the group
                if let Some(releases) = &group.releases {
                    for release in releases.iter().take(3) {
                        let cover = release.get_coverart().front().res_500().execute().await?;
                        if let CoverartResponse::Url(url) = cover {
                            info!("Found cover art URL from release in group");
                            return Ok(Some(url));
                        }
                    }
                }
            }
        }

        // Try direct release search
        debug!("Trying direct release search");
        let mut builder = ReleaseSearchQuery::query_builder();
        builder.release(album.as_ref());

        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
        }

        let results = Release::search(builder.build()).execute().await?;

        if !results.entities.is_empty() {
            debug!("Found {} direct release results", results.entities.len());

            for release in results.entities.iter().take(3) {
                let cover = release.get_coverart().front().res_500().execute().await?;
                if let CoverartResponse::Url(url) = cover {
                    info!("Found cover art URL from direct release search");
                    return Ok(Some(url));
                }
            }
        }

        info!("No cover art found for album: {}", album.as_ref());
        Ok(None)
    }

    async fn search_track<S: AsRef<str>>(
        &self,
        track: S,
        artists: &[S],
        duration_ms: Option<u128>,
    ) -> Result<Option<String>, CoverArtError> {
        info!("Searching MusicBrainz for track: {}", track.as_ref());

        let mut builder = RecordingSearchQuery::query_builder();
        builder.recording(track.as_ref());

        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
            debug!("Including artist in search: {}", artist.as_ref());
        }

        if let Some(duration) = duration_ms {
            let duration_range =
                format!("[{} TO {}]", duration.saturating_sub(3000), duration + 3000);
            builder.and().duration(duration_range.as_str());
            debug!("Using duration range: {}", duration_range);
        }

        let results = Recording::search(builder.build())
            .with_releases()
            .execute()
            .await?;

        if !results.entities.is_empty() {
            debug!("Found {} recording results", results.entities.len());

            for recording in results.entities.iter().take(3) {
                if let Some(releases) = &recording.releases {
                    for release in releases.iter().take(3) {
                        // Try release cover
                        let cover = release.get_coverart().front().res_500().execute().await?;
                        if let CoverartResponse::Url(url) = cover {
                            info!("Found cover art URL from release for recording");
                            return Ok(Some(url));
                        }

                        // Try release group cover
                        if let Some(rg) = &release.release_group {
                            let cover = rg.get_coverart().front().res_500().execute().await?;
                            if let CoverartResponse::Url(url) = cover {
                                info!("Found cover art URL from release group for recording");
                                return Ok(Some(url));
                            }
                        }
                    }
                }
            }
        }

        info!("No cover art found for track: {}", track.as_ref());
        Ok(None)
    }
}

#[async_trait]
impl CoverArtProvider for MusicbrainzProvider {
    fn name(&self) -> &'static str {
        "musicbrainz"
    }

    fn supports_source_type(&self, _source: &ArtSource) -> bool {
        true // Works with any source type
    }

    async fn process(
        &self,
        _source: ArtSource,
        metadata: &Metadata,
    ) -> Result<Option<CoverResult>, CoverArtError> {
        info!("MusicBrainz provider searching for cover art");
        debug!(
            "Metadata: album={:?}, title={:?}",
            metadata.album_name(),
            metadata.title()
        );

        let artists = metadata.artists().unwrap_or_default();
        let album_artists = metadata.album_artists().unwrap_or_default();

        // Try album search first
        if let Some(album) = metadata.album_name() {
            let search_artists = if !album_artists.is_empty() {
                album_artists.as_slice()
            } else {
                artists.as_slice()
            };

            if !search_artists.is_empty() {
                if let Some(url) = self.search_album(album, search_artists).await? {
                    return Ok(Some(CoverResult {
                        url,
                        provider: self.name().to_string(),
                        expiration: None,
                    }));
                }
            }
        }

        // Fall back to track search
        if let Some(title) = metadata.title() {
            if !artists.is_empty() {
                let duration = metadata.length().map(|d| d.as_millis());

                if let Some(url) = self
                    .search_track(title, artists.as_slice(), duration)
                    .await?
                {
                    return Ok(Some(CoverResult {
                        url,
                        provider: self.name().to_string(),
                        expiration: None,
                    }));
                }
            }
        }

        info!("MusicBrainz provider found no cover art");
        Ok(None)
    }
}
