use std::collections::HashSet;

use crate::{
    context::Context,
    cover::{cache::Cache, REQWEST_CLIENT},
};
use serde::Deserialize;

#[derive(Debug)]
pub struct MusicBrainzProvider {
    cache: Cache,
}

impl MusicBrainzProvider {
    pub fn new() -> Self {
        Self {
            cache: Cache::new("musicbrainz"),
        }
    }

    pub async fn get_cover_url(&self, context: &Context) -> Option<String> {
        let title = context.title();
        //Split artist names by comma and & and trim
        let artists = context.artists().map(|artists| {
            let mut splitted_artists = HashSet::new();

            for artist in artists.iter() {
                splitted_artists.extend(artist.split(',').map(|s| s.trim()));
                splitted_artists.extend(artist.split('&').map(|s| s.trim()));
                splitted_artists.extend(artist.split(" and ").map(|s| s.trim()));
                splitted_artists.extend(artist.split(" feat. ").map(|s| s.trim()));
                splitted_artists.extend(artist.split(" vs ").map(|s| s.trim()));
            }

            splitted_artists.into_iter().collect::<Vec<_>>()
        });
        let album = context.album_name();
        let album_artists = context.album_artists();

        let mut all_artists: Option<Vec<&str>> = match (&artists, &album_artists) {
            (Some(a), Some(b)) => {
                let mut set = HashSet::new();
                for artist in a.iter() {
                    set.insert(artist);
                }
                for artist in b.iter() {
                    set.insert(artist);
                }
                Some(set.into_iter().cloned().collect())
            }
            (Some(a), None) => Some(a.clone()),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        };

        all_artists.as_mut().map(|artists| artists.sort());

        let cache_key_raw = match (&title, &album, &artists, &album_artists) {
            (_, Some(album), _, Some(album_artists)) => {
                format!("{}-{}", album_artists.join(", "), album)
            }
            (_, Some(album), Some(artists), None) => format!("{}-{}", artists.join(", "), album),
            (Some(title), Some(album), _, _) => format!("{}-{}", title, album),
            (Some(title), _, Some(artists), _) => format!("{}-{}", artists.join(", "), title),
            (_, _, _, _) => return None,
        };

        let cache_key = sha256::digest(cache_key_raw.as_bytes());

        if let Some(url) = self.cache.get(&cache_key) {
            return Some(url);
        }

        let mut similar_releases: Vec<ReleaseSearch> = Vec::new();

        if let (Some(title), Some(artists), Some(album)) = (title, artists, album) {
            for artist in artists {
                let recordings = match Self::search_recording(
                    &Query::new()
                        .recording(title)
                        .artist(artist)
                        .release(album)
                        .build(),
                )
                .await
                {
                    Ok(recordings) => recordings,
                    Err(e) => {
                        log::warn!("Error while searching for recordings: {}", e);
                        return None;
                    }
                };
                for recording in recordings.recordings {
                    for release in recording.releases {
                        let mut release = release.clone();
                        release.artist_credit = recording.artist_credit.clone();
                        similar_releases.push(release);
                    }
                }
            }
        }

        if let (Some(album), Some(all_artists)) = (album, all_artists.as_ref()) {
            for artist in all_artists {
                let releases =
                    match Self::search_release(&Query::new().release(album).artist(artist).build())
                        .await
                    {
                        Ok(releases) => releases,
                        Err(e) => {
                            log::warn!("Error while searching for releases: {}", e);
                            return None;
                        }
                    };
                for release in releases.releases {
                    similar_releases.push(release);
                }
            }
        }

        if let Some(album) = album {
            let releases = match Self::search_release(&Query::new().release(album).build()).await {
                Ok(releases) => releases,
                Err(e) => {
                    log::warn!("Error while searching for releases: {}", e);
                    return None;
                }
            };
            for release in releases.releases {
                similar_releases.push(release);
            }
        }

        log::debug!("Found {} similar releases", similar_releases.len());

        similar_releases.retain(|release| {
            let release_title_lower = release.title.to_lowercase();

            if let Some(album) = album {
                let album_lower = album.to_lowercase();
                let jarowinkler = strsim::jaro_winkler(&album_lower, &release_title_lower);
                if !(album_lower.contains(&release_title_lower) || jarowinkler > 0.9) {
                    return false;
                }
            }

            // retain if there is at least one artist that matches
            for artist in all_artists.as_ref().unwrap_or(&Vec::new()) {
                let artist_lower = artist.to_lowercase();
                if release.artist_credit.iter().any(|ac| {
                    ac.name.to_lowercase().contains(&artist_lower)
                        || artist_lower.contains(&ac.name.to_lowercase())
                        || { strsim::jaro_winkler(&artist_lower, &ac.name.to_lowercase()) > 0.8 }
                }) {
                    return true;
                }
            }

            false
        });

        similar_releases.sort_by(|a, b| {
            let a_title = a.title.to_lowercase();
            let b_title = b.title.to_lowercase();

            let album = album.unwrap_or_default().to_lowercase();

            let a_title_sim = strsim::jaro_winkler(&a_title, &album);
            let b_title_sim = strsim::jaro_winkler(&b_title, &album);

            if a_title_sim > b_title_sim {
                return std::cmp::Ordering::Less;
            } else if a_title_sim < b_title_sim {
                return std::cmp::Ordering::Greater;
            }

            let mut a_score = 0.0;
            let mut b_score = 0.0;

            if let Some(ref all_artists) = all_artists {
                for a_artist_credit in &a.artist_credit {
                    for artist in all_artists.iter() {
                        a_score += strsim::jaro_winkler(&a_artist_credit.name, artist);
                    }
                }

                for b_artist_credit in &b.artist_credit {
                    for artist in all_artists.iter() {
                        b_score += strsim::jaro_winkler(&b_artist_credit.name, artist);
                    }
                }
            }

            if a_score > b_score {
                return std::cmp::Ordering::Less;
            } else if a_score < b_score {
                return std::cmp::Ordering::Greater;
            }

            std::cmp::Ordering::Equal
        });

        for release in similar_releases {
            log::debug!("Checking release cover art existance: {}", release.title);

            let release_id = release.id;

            match Self::get_release(&release_id).await {
                Ok(release) => {
                    if !release.cover_art_archive.front {
                        continue;
                    }
                }
                Err(e) => {
                    log::warn!("Error while getting release: {}", e);
                    continue;
                }
            }
            let cover_url = format!(
                "https://coverartarchive.org/release/{}/front-250",
                release_id
            );

            self.cache.set(&cache_key, &cover_url.clone());

            return Some(cover_url);
        }
        None
    }

    pub async fn search_recording(query: &str) -> Result<RecordingSearchReponse, reqwest::Error> {
        let url = format!(
            "https://musicbrainz.org/ws/2/recording/?query={}&fmt=json&limit=5",
            query
        );
        log::info!("searching for recording: {}", url);
        let res = REQWEST_CLIENT.get(&url).send().await?;
        let response = res.json::<RecordingSearchReponse>().await?;
        Ok(response)
    }
    pub async fn search_release(query: &str) -> Result<ReleaseSearchResponse, reqwest::Error> {
        let url = format!(
            "https://musicbrainz.org/ws/2/release/?query={}&fmt=json&limit=5",
            query
        );
        log::info!("searching for release: {}", url);
        let res = REQWEST_CLIENT.get(&url).send().await?;
        let response = res.json::<ReleaseSearchResponse>().await?;
        Ok(response)
    }

    pub async fn get_release(id: &str) -> Result<Release, reqwest::Error> {
        let url = format!("https://musicbrainz.org/ws/2/release/{}?fmt=json", id);
        log::info!("getting release: {}", url);
        let res = REQWEST_CLIENT.get(&url).send().await?;
        let response = res.json::<Release>().await?;
        Ok(response)
    }
}

pub struct Query {
    recording: Option<String>,
    release: Option<String>,
    artist: Option<String>,
}

impl Query {
    pub fn new() -> Self {
        Self {
            recording: None,
            release: None,
            artist: None,
        }
    }

    pub fn recording(&mut self, recording: &str) -> &mut Self {
        self.recording = Some(recording.to_owned());
        self
    }

    pub fn release(&mut self, album: &str) -> &mut Self {
        self.release = Some(album.to_owned());
        self
    }

    pub fn artist(&mut self, artist: &str) -> &mut Self {
        self.artist = Some(artist.to_owned());
        self
    }

    pub fn build(&self) -> String {
        let mut query = String::new();

        if let Some(ref recording) = self.recording {
            query.push_str(&format!("recording:{}", recording));
        }

        if let Some(ref release) = self.release {
            if !query.is_empty() {
                query.push_str(" AND ");
            }
            query.push_str(&format!("release:{}", release));
        }

        if let Some(ref artist) = self.artist {
            if !query.is_empty() {
                query.push_str(" AND ");
            }
            query.push_str(&format!("artist:{}", artist));
        }

        query
    }
}

#[derive(Debug, Deserialize)]
pub struct ReleaseSearchResponse {
    #[serde(default)]
    pub releases: Vec<ReleaseSearch>,
}

#[derive(Debug, Deserialize)]
pub struct RecordingSearchReponse {
    #[serde(default)]
    pub recordings: Vec<RecordingSearch>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RecordingSearch {
    pub id: String,
    pub title: String,
    #[serde(rename = "artist-credit", default)]
    pub artist_credit: Vec<ArtistCredit>,
    #[serde(default)]
    pub releases: Vec<ReleaseSearch>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ArtistCredit {
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReleaseSearch {
    pub id: String,
    pub title: String,
    #[serde(rename = "artist-credit", default)]
    pub artist_credit: Vec<ArtistCredit>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Release {
    pub id: String,
    #[serde(rename = "cover-art-archive")]
    pub cover_art_archive: CoverArtArchive,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CoverArtArchive {
    pub front: bool,
}
