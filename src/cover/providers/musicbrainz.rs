use async_trait::async_trait;
use log::{debug, info, trace, warn};
use reqwest::Client;
use serde::Deserialize;

use super::{CoverArtProvider, CoverResult, create_shared_client};
use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;
use crate::metadata::MetadataSource;

const MUSICBRAINZ_API: &str = "https://musicbrainz.org/ws/2";
const COVERART_API: &str = "https://coverartarchive.org";

#[derive(Debug, Deserialize)]
struct MusicBrainzResponse<T> {
    count: u32,
    #[serde(rename = "recordings", alias = "releases", alias = "release-groups")]
    entities: Vec<T>,
}

#[derive(Debug, Deserialize)]
struct Recording {
    id: String,
    releases: Option<Vec<Release>>,
    #[serde(default)]
    score: u8,
}

#[derive(Debug, Deserialize)]
struct Release {
    id: String,
    #[serde(rename = "release-group")]
    release_group: Option<ReleaseGroup>,
    #[serde(default)]
    score: u8,
}

#[derive(Debug, Deserialize)]
struct ReleaseGroup {
    id: String,
    #[serde(default)]
    score: u8,
}

#[derive(Clone)]
pub struct MusicbrainzProvider {
    client: Client,
}

impl Default for MusicbrainzProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MusicbrainzProvider {
    const MIN_SCORE: u8 = 95;
    
    // Thumbnail sizes to try in order of preference
    const THUMBNAIL_SIZES: [u16; 3] = [500, 250, 1200];

    pub fn new() -> Self {
        info!("Initializing MusicBrainz provider");
        Self {
            client: create_shared_client(),
        }
    }

    async fn get_cover_art(&self, entity_type: &str, mbid: &str) -> Result<Option<String>, CoverArtError> {
        trace!("Attempting to fetch cover art for {} ({})", entity_type, mbid);
        
        for size in Self::THUMBNAIL_SIZES {
            let url = format!("{}/{}/{}/front-{}", COVERART_API, entity_type, mbid, size);
            debug!("Requesting cover art from: {} ({}px)", url, size);
            
            match self.client.get(&url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        debug!("Successfully found cover art for {} at {}px", mbid, size);
                        return Ok(Some(url));
                    }
                }
                Err(e) => {
                    if e.is_timeout() {
                        warn!("Cover art request timed out for {} ({})", entity_type, mbid);
                        return Ok(None);
                    }
                    warn!("Network error fetching cover art for {} ({}): {}", entity_type, mbid, e);
                    continue;
                }
            }
        }
        
        debug!("No suitable cover art found for {} ({})", entity_type, mbid);
        Ok(None)
    }

    async fn try_cover_art_sources(&self, sources: Vec<(String, String)>) -> Result<Option<String>, CoverArtError> {
        let source_count = sources.len();
        debug!("Processing {} cover art sources in priority order", source_count);
        
        for (idx, (entity_type, id)) in sources.into_iter().enumerate() {
            trace!("Trying source {}/{}: {} ({})", idx + 1, source_count, entity_type, id);
            match self.get_cover_art(&entity_type, &id).await {
                Ok(Some(url)) => {
                    debug!(
                        "Found valid cover art URL from source {}/{}: {}",
                        idx + 1, 
                        source_count, 
                        url
                    );
                    return Ok(Some(url));
                }
                Ok(None) => {
                    trace!("No cover art found for source {}/{}", idx + 1, source_count);
                    continue;
                }
                Err(e) => {
                    warn!("Error fetching cover art from source {}/{}: {}", idx + 1, source_count, e);
                    continue;
                }
            }
        }
        
        debug!("No valid cover art found from any source");
        Ok(None)
    }

    async fn search_album<S: AsRef<str>>(
        &self,
        album: S,
        artists: &[S],
    ) -> Result<Option<String>, CoverArtError> {
        debug!("Searching MusicBrainz for album: {} by [{}]", 
            album.as_ref(),
            artists.iter().map(|a| a.as_ref()).collect::<Vec<_>>().join(", ")
        );

        let mut query = format!("release-group:{}", album.as_ref());
        if let Some(artist) = artists.first() {
            query.push_str(&format!(" AND artist:{}", artist.as_ref()));
        }

        let encoded_query = urlencoding::encode(&query);
        trace!("Encoded MusicBrainz query: {}", encoded_query);
        
        let (release_groups, releases) = futures::join!(
            self.client.get(format!("{}/release-group?query={}&limit=5&fmt=json", 
                MUSICBRAINZ_API, encoded_query))
                .send(),
            self.client.get(format!("{}/release?query={}&limit=5&fmt=json", 
                MUSICBRAINZ_API, encoded_query))
                .send()
        );

        let mut cover_sources = Vec::new();

        if let Ok(response) = release_groups {
            if let Ok(data) = response.json::<MusicBrainzResponse<ReleaseGroup>>().await {
                debug!("Found {} release groups (filtering by score >= {})", data.count, Self::MIN_SCORE);
                cover_sources.extend(
                    data.entities.iter()
                        .filter(|group| group.score >= Self::MIN_SCORE)
                        .map(|group| {
                            trace!("Adding release group to sources: {} (score: {})", group.id, group.score);
                            ("release-group".to_string(), group.id.clone())
                        })
                );
            }
        }

        if let Ok(response) = releases {
            if let Ok(data) = response.json::<MusicBrainzResponse<Release>>().await {
                debug!("Found {} releases (filtering by score >= {})", data.count, Self::MIN_SCORE);
                cover_sources.extend(
                    data.entities.iter()
                        .filter(|release| release.score >= Self::MIN_SCORE)
                        .map(|release| {
                            trace!("Adding release to sources: {} (score: {})", release.id, release.score);
                            ("release".to_string(), release.id.clone())
                        })
                );
            }
        }

        if cover_sources.is_empty() {
            debug!("No sources found meeting minimum score threshold of {}", Self::MIN_SCORE);
        } else {
            debug!("Found {} potential cover art sources", cover_sources.len());
        }

        self.try_cover_art_sources(cover_sources).await
    }

    async fn search_track<S: AsRef<str>>(
        &self,
        track: S,
        artists: &[S],
        duration_ms: Option<u128>,
    ) -> Result<Option<String>, CoverArtError> {
        debug!("Searching MusicBrainz for track: {} by [{}]", 
            track.as_ref(),
            artists.iter().map(|a| a.as_ref()).collect::<Vec<_>>().join(", ")
        );

        let mut query = format!("recording:{}", track.as_ref());
        
        if let Some(artist) = artists.first() {
            query.push_str(&format!(" AND artist:{}", artist.as_ref()));
        }

        if let Some(duration) = duration_ms {
            let duration_range = format!(
                " AND dur:[{} TO {}]",
                duration.saturating_sub(3000),
                duration + 3000
            );
            query.push_str(&duration_range);
            trace!("Added duration range to query: {}", duration_range);
        }

        let encoded_query = urlencoding::encode(&query);
        trace!("Encoded MusicBrainz query: {}", encoded_query);

        let url = format!(
            "{}/recording?query={}&limit=5&fmt=json",
            MUSICBRAINZ_API,
            encoded_query
        );

        if let Ok(response) = self.client.get(&url).send().await {
            if let Ok(data) = response.json::<MusicBrainzResponse<Recording>>().await {
                debug!("Found {} recordings (filtering by score >= {})", data.count, Self::MIN_SCORE);
                let mut cover_sources = Vec::new();

                for recording in data.entities.iter().filter(|r| r.score >= Self::MIN_SCORE) {
                    trace!("Processing recording: {} (score: {})", recording.id, recording.score);
                    if let Some(releases) = &recording.releases {
                        for release in releases.iter().take(2) {
                            trace!("Adding release to sources: {} (score: {})", release.id, release.score);
                            cover_sources.push(("release".to_string(), release.id.clone()));
                            if let Some(group) = &release.release_group {
                                trace!("Adding release group to sources: {} (score: {})", group.id, group.score);
                                cover_sources.push(("release-group".to_string(), group.id.clone()));
                            }
                        }
                    }
                }

                if cover_sources.is_empty() {
                    debug!("No sources found meeting minimum score threshold of {}", Self::MIN_SCORE);
                } else {
                    debug!("Found {} potential cover art sources", cover_sources.len());
                }

                return self.try_cover_art_sources(cover_sources).await;
            }
        }

        debug!("No recordings found matching the search criteria");
        Ok(None)
    }
}

#[async_trait]
impl CoverArtProvider for MusicbrainzProvider {
    fn name(&self) -> &'static str {
        "musicbrainz"
    }

    fn supports_source_type(&self, _source: &ArtSource) -> bool {
        true // MusicBrainz can work with any source type as it uses metadata
    }

    async fn process(
        &self,
        _source: ArtSource,
        metadata_source: &MetadataSource
    ) -> Result<Option<CoverResult>, CoverArtError> {
        info!("Processing metadata with MusicBrainz provider");
        trace!(
            "Metadata details: album={:?}, title={:?}, artists={:?}, album_artists={:?}, length={:?}",
            metadata_source.album(),
            metadata_source.title(),
            metadata_source.artists(),
            metadata_source.album_artists(),
            metadata_source.length()
        );

        let mut cover_sources = Vec::new();

        if let Some(id) = metadata_source.musicbrainz_release_group_id() {
            debug!("Found MusicBrainz Release Group ID: {}", id);
            cover_sources.push(("release-group".to_string(), id));
        }
        if let Some(id) = metadata_source.musicbrainz_album_id() {
            debug!("Found MusicBrainz Album/Release ID: {}", id);
            cover_sources.push(("release".to_string(), id));
        }

        if !cover_sources.is_empty() {
            debug!("Attempting fetch using {} direct MusicBrainz IDs", cover_sources.len());
            if let Some(url) = self.try_cover_art_sources(cover_sources).await? {
                info!(
                    "Successfully found cover art via direct MusicBrainz ID: {}",
                    url
                );
                return Ok(Some(CoverResult {
                    url,
                    provider: self.name().to_string(),
                    expiration: None,
                }));
            }
            debug!("Fetching via direct ID failed or yielded no results.");
        } else {
            debug!("No direct MusicBrainz IDs found in metadata.");
        }

        info!("Falling back to MusicBrainz search based on metadata");
        let artists = metadata_source.artists().unwrap_or_default();
        let album_artists = metadata_source.album_artists().unwrap_or_default();

        if let Some(album) = metadata_source.album() {
            let search_artists_refs: Vec<&String> = if !album_artists.is_empty() {
                trace!("Using album artists for search: {:?}", album_artists);
                album_artists.iter().collect()
            } else {
                trace!("Using track artists for search: {:?}", artists);
                artists.iter().collect()
            };

            if !search_artists_refs.is_empty() {
                debug!("Attempting album-based search for '{}' with artists", album);
                if let Some(url) = self.search_album(&album, &search_artists_refs).await? {
                    info!(
                        "Successfully found cover art via album search: {}",
                        url
                    );
                    return Ok(Some(CoverResult {
                        url,
                        provider: self.name().to_string(),
                        expiration: None,
                    }));
                }
                debug!("Album search yielded no results");
            } else {
                debug!("No artists available for album search");
            }
        }

        if let Some(title) = metadata_source.title() {
            if !artists.is_empty() {
                let duration = metadata_source.length().map(|d| d.as_millis());
                debug!("Attempting track-based search for '{}' with artists", title);
                trace!("Track duration: {:?}ms", duration);
                let artists_refs: Vec<&String> = artists.iter().collect();

                if let Some(url) = self
                    .search_track(&title, &artists_refs, duration)
                    .await?
                {
                    info!(
                        "Successfully found cover art via track search: {}",
                        url
                    );
                    return Ok(Some(CoverResult {
                        url,
                        provider: self.name().to_string(),
                        expiration: None,
                    }));
                }
                debug!("Track search yielded no results");
            } else {
                debug!("No artists available for track search");
            }
        }

        debug!("MusicBrainz provider found no suitable cover art");
        Ok(None)
    }
}
