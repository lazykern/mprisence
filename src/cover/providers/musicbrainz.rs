use async_trait::async_trait;
use log::{debug, info, warn};
use mpris::Metadata;
use reqwest::Client;
use serde::Deserialize;

use super::{CoverArtProvider, CoverResult, create_shared_client};
use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;

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

#[derive(Debug, Deserialize)]
struct CoverArtResponse {
    images: Vec<CoverArtImage>,
}

#[derive(Debug, Deserialize)]
struct CoverArtImage {
    front: bool,
    thumbnails: CoverArtThumbnails,
}

#[derive(Debug, Deserialize)]
struct CoverArtThumbnails {
    #[serde(rename = "500")]
    large: String,
}

#[derive(Clone)]
pub struct MusicbrainzProvider {
    client: Client,
}

impl MusicbrainzProvider {
    // Add constant for minimum score threshold
    const MIN_SCORE: u8 = 90; // Only accept matches with 90% or higher confidence
    
    // Default to 500px thumbnails as a good balance between quality and size
    // Can be changed to 250 for smaller thumbnails or 1200 for high quality
    const THUMBNAIL_SIZE: u16 = 500;

    pub fn new() -> Self {
        debug!("Creating new MusicBrainz provider");
        Self {
            client: create_shared_client(),
        }
    }

    async fn get_cover_art(&self, entity_type: &str, mbid: &str) -> Result<Option<String>, CoverArtError> {
        // Try primary size first
        let primary_url = format!("{}/{}/{}/front-{}", COVERART_API, entity_type, mbid, Self::THUMBNAIL_SIZE);
        debug!("Attempting to fetch cover art from: {} ({}px)", primary_url, Self::THUMBNAIL_SIZE);
        
        match self.client.get(&primary_url).send().await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    debug!("Successfully found cover art for {} ({}) at {}px", entity_type, mbid, Self::THUMBNAIL_SIZE);
                    return Ok(Some(primary_url));
                }
                
                // If primary size fails with 404, try fallback sizes in order: 500 -> 250 -> 1200
                if status.as_u16() == 404 {
                    debug!("{}px thumbnail not available, trying fallback sizes", Self::THUMBNAIL_SIZE);
                    
                    // Fixed fallback chain: 500 -> 250 -> 1200
                    let fallback_sizes = vec![250, 1200];
                    
                    // Try each fallback size
                    for size in fallback_sizes {
                        let fallback_url = format!("{}/{}/{}/front-{}", COVERART_API, entity_type, mbid, size);
                        debug!("Trying fallback size: {}px", size);
                        
                        match self.client.get(&fallback_url).send().await {
                            Ok(fallback_response) => {
                                if fallback_response.status().is_success() {
                                    debug!("Successfully found cover art at fallback size: {}px", size);
                                    return Ok(Some(fallback_url));
                                }
                            }
                            Err(e) => {
                                warn!("Error trying fallback size {}px: {}", size, e);
                            }
                        }
                    }
                }
                
                debug!("No cover art found for {} ({}) at any size - Status: {}", entity_type, mbid, status);
                Ok(None)
            }
            Err(e) => {
                if e.is_timeout() {
                    warn!("Cover art request timed out for {} ({})", entity_type, mbid);
                    Ok(None)
                } else {
                    warn!("Network error fetching cover art for {} ({}): {}", entity_type, mbid, e);
                    Err(CoverArtError::NetworkError(e.to_string()))
                }
            }
        }
    }

    async fn try_cover_art_sources(&self, sources: Vec<(String, String)>) -> Result<Option<String>, CoverArtError> {
        let source_count = sources.len();
        debug!("Trying {} cover art sources in priority order", source_count);
        
        // Try sources sequentially to respect MusicBrainz's relevance ordering
        for (idx, (entity_type, id)) in sources.into_iter().enumerate() {
            debug!("Trying source {} of {}: {} ({})", idx + 1, source_count, entity_type, id);
            match self.get_cover_art(&entity_type, &id).await {
                Ok(Some(url)) => {
                    info!("Found valid cover art URL from source {}: {}", idx + 1, url);
                    return Ok(Some(url));
                }
                Ok(None) => {
                    debug!("No cover art found for source {}", idx + 1);
                    continue;
                }
                Err(e) => {
                    warn!("Error fetching cover art for source {}: {}", idx + 1, e);
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
        info!("Searching MusicBrainz for album: {} by [{}]", 
            album.as_ref(),
            artists.iter().map(|a| a.as_ref()).collect::<Vec<_>>().join(", ")
        );

        let mut query = format!("release-group:{}", album.as_ref());
        if let Some(artist) = artists.first() {
            query.push_str(&format!(" AND artist:{}", artist.as_ref()));
        }

        let encoded_query = urlencoding::encode(&query);
        debug!("Encoded query: {}", encoded_query);
        
        // Fetch both release groups and releases in parallel, with minimum score
        let (release_groups, releases) = futures::join!(
            self.client.get(&format!("{}/release-group?query={}&limit=2&fmt=json&score={}", 
                MUSICBRAINZ_API, encoded_query, Self::MIN_SCORE))
                .send(),
            self.client.get(&format!("{}/release?query={}&limit=2&fmt=json&score={}", 
                MUSICBRAINZ_API, encoded_query, Self::MIN_SCORE))
                .send()
        );

        let mut cover_sources = Vec::new();

        // Process release groups
        if let Ok(response) = release_groups {
            if let Ok(data) = response.json::<MusicBrainzResponse<ReleaseGroup>>().await {
                debug!("Found {} release groups (limited to 2)", data.count);
                cover_sources.extend(
                    data.entities.iter()
                        .map(|group| {
                            debug!("Adding release group: {} (score: {})", group.id, group.score);
                            ("release-group".to_string(), group.id.clone())
                        })
                );
            }
        }

        // Process releases
        if let Ok(response) = releases {
            if let Ok(data) = response.json::<MusicBrainzResponse<Release>>().await {
                debug!("Found {} releases (limited to 2)", data.count);
                cover_sources.extend(
                    data.entities.iter()
                        .map(|release| {
                            debug!("Adding release: {} (score: {})", release.id, release.score);
                            ("release".to_string(), release.id.clone())
                        })
                );
            }
        }

        self.try_cover_art_sources(cover_sources).await
    }

    async fn search_track<S: AsRef<str>>(
        &self,
        track: S,
        artists: &[S],
        duration_ms: Option<u128>,
    ) -> Result<Option<String>, CoverArtError> {
        info!("Searching MusicBrainz for track: {} by [{}]", 
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
            debug!("Added duration range to query: {}", duration_range);
        }

        let encoded_query = urlencoding::encode(&query);
        debug!("Encoded query: {}", encoded_query);

        let url = format!(
            "{}/recording?query={}&limit=2&fmt=json&score={}",
            MUSICBRAINZ_API,
            encoded_query,
            Self::MIN_SCORE
        );

        if let Ok(response) = self.client.get(&url).send().await {
            if let Ok(data) = response.json::<MusicBrainzResponse<Recording>>().await {
                debug!("Found {} recordings (limited to 2)", data.count);
                let mut cover_sources = Vec::new();

                for (idx, recording) in data.entities.iter().enumerate() {
                    debug!("Processing recording {} (id: {}, score: {})", idx + 1, recording.id, recording.score);
                    if let Some(releases) = &recording.releases {
                        for (release_idx, release) in releases.iter().take(2).enumerate() {
                            debug!("Processing release {} (id: {}, score: {})", release_idx + 1, release.id, release.score);
                            cover_sources.push(("release".to_string(), release.id.clone()));
                            if let Some(group) = &release.release_group {
                                debug!("Adding release group: {} (score: {})", group.id, group.score);
                                cover_sources.push(("release-group".to_string(), group.id.clone()));
                            }
                        }
                    }
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
        true // Works with any source type
    }

    async fn process(
        &self,
        _source: ArtSource,
        metadata: &Metadata,
    ) -> Result<Option<CoverResult>, CoverArtError> {
        info!("MusicBrainz provider processing metadata");
        debug!(
            "Full metadata: album={:?}, title={:?}, artists={:?}, album_artists={:?}, length={:?}",
            metadata.album_name(),
            metadata.title(),
            metadata.artists(),
            metadata.album_artists(),
            metadata.length()
        );

        let artists = metadata.artists().unwrap_or_default();
        let album_artists = metadata.album_artists().unwrap_or_default();

        // Try album search first
        if let Some(album) = metadata.album_name() {
            let search_artists = if !album_artists.is_empty() {
                debug!("Using album artists for search: {:?}", album_artists);
                album_artists.as_slice()
            } else {
                debug!("Using track artists for search: {:?}", artists);
                artists.as_slice()
            };

            if !search_artists.is_empty() {
                debug!("Attempting album search for '{}' with artists {:?}", album, search_artists);
                if let Some(url) = self.search_album(album, search_artists).await? {
                    info!("Found cover art via album search: {}", url);
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

        // Fall back to track search
        if let Some(title) = metadata.title() {
            if !artists.is_empty() {
                let duration = metadata.length().map(|d| d.as_millis());
                debug!("Attempting track search for '{}' with artists {:?} and duration {:?}ms", title, artists, duration);

                if let Some(url) = self
                    .search_track(title, artists.as_slice(), duration)
                    .await?
                {
                    info!("Found cover art via track search: {}", url);
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

        info!("MusicBrainz provider found no cover art");
        Ok(None)
    }
}
