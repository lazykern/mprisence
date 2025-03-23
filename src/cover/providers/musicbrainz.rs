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

use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;
use super::{CoverArtProvider, CoverResult};

#[derive(Clone)]
pub struct MusicbrainzProvider;

impl MusicbrainzProvider {
    pub fn new() -> Self {
        debug!("Creating new MusicBrainz provider");
        Self {}
    }

    // Unified search function that handles both release groups and releases
    async fn search_album<S: AsRef<str>>(
        &self,
        album: S,
        artists: &[S],
    ) -> Result<Option<String>, CoverArtError> {
        info!("Searching MusicBrainz for album: {}", album.as_ref());
        
        // First try release groups
        let mut builder = ReleaseGroupSearchQuery::query_builder();
        builder.release_group(album.as_ref());
        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
            debug!("Including artist in search: {}", artist.as_ref());
        }

        // Perform the search and check for cover art
        let results = ReleaseGroup::search(builder.build()).execute().await?;
        if !results.entities.is_empty() {
            debug!("Found {} release group results", results.entities.len());
        
            // Check first couple results
            for group in results.entities.iter().take(2) {
                // Try the release group cover directly
                let cover = group.get_coverart().front().res_500().execute().await?;
                if let CoverartResponse::Url(url) = cover {
                    info!("Found cover art URL from release group");
                    return Ok(Some(url));
                }

                // Try covers from releases in the group
                if let Some(releases) = &group.releases {
                    for release in releases.iter().take(2) {
                        let cover = release.get_coverart().front().res_500().execute().await?;
                        if let CoverartResponse::Url(url) = cover {
                            info!("Found cover art URL from release in group");
                            return Ok(Some(url));
                        }
                    }
                }
            }
        }

        // If no release group covers found, try direct release search
        debug!("Trying direct release search");
        let mut builder = ReleaseSearchQuery::query_builder();
        builder.release(album.as_ref());
        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
        }

        let results = Release::search(builder.build()).execute().await?;
        if !results.entities.is_empty() {
            debug!("Found {} direct release results", results.entities.len());
            
            // Check first couple of releases
            for release in results.entities.iter().take(2) {
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

    // Search by track/recording
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

        // Add duration if available for more accurate matching
        if let Some(duration) = duration_ms {
            let duration_range = format!("[{} TO {}]", duration.saturating_sub(3000), duration + 3000);
            builder.and().duration(duration_range.as_str());
            debug!("Using duration range: {}", duration_range);
        }

        let results = Recording::search(builder.build())
            .with_releases()
            .execute()
            .await?;
            
        if !results.entities.is_empty() {
            debug!("Found {} recording results", results.entities.len());

            // Check each recording and its releases
            for recording in results.entities.iter().take(2) {
                if let Some(releases) = &recording.releases {
                    for release in releases.iter().take(2) {
                        // Try release cover first
                        let cover = release.get_coverart().front().res_500().execute().await?;
                        if let CoverartResponse::Url(url) = cover {
                            info!("Found cover art URL from release for recording");
                            return Ok(Some(url));
                        }

                        // Try release group cover if available
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
        // MusicBrainz doesn't process source data, it uses metadata to look up cover art
        // It can be used as a fallback for any source type
        true
    }
    
    async fn process(&self, _source: ArtSource, metadata: &Metadata) -> Result<Option<CoverResult>, CoverArtError> {
        info!("MusicBrainz provider searching for cover art");
        debug!("Metadata: album={:?}, title={:?}", metadata.album_name(), metadata.title());
        
        let artists = metadata.artists().unwrap_or_default();
        let artists = artists.as_slice();
        let album_artists = metadata.album_artists().unwrap_or_default();
        let album_artists = album_artists.as_slice();
        
        // Try album search first if we have album metadata
        if let Some(album) = metadata.album_name() {
            info!("Attempting album search for: {}", album);
            let search_artists = if !album_artists.is_empty() {
                album_artists
            } else {
                artists
            };

            if !search_artists.is_empty() {
                if let Some(url) = self.search_album(album, search_artists).await? {
                    info!("Found cover art URL through album search");
                    return Ok(Some(CoverResult {
                        url,
                        provider: self.name().to_string(),
                        expiration: None, // MusicBrainz URLs don't expire
                    }));
                }
            }
        }

        // Fall back to track search
        if let Some(title) = metadata.title() {
            info!("Falling back to track search for: {}", title);
            if !artists.is_empty() {
                let duration = metadata.length().map(|d| d.as_millis());
                
                if let Some(url) = self.search_track(title, artists, duration).await? {
                    info!("Found cover art URL through track search");
                    return Ok(Some(CoverResult {
                        url,
                        provider: self.name().to_string(),
                        expiration: None, // MusicBrainz URLs don't expire
                    }));
                }
            }
        }

        info!("MusicBrainz provider found no cover art");
        Ok(None)
    }
}
