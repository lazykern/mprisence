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
        let album = context.album_name();
        let artists = context.artists();
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

        let mut similar_releases: Vec<Release> = Vec::new();

        if let (Some(title), Some(artists)) = (title, artists) {
            for artist in artists {
                let recordings =
                    Self::search_recording(&Query::new().recording(title).artist(artist).build())
                        .await
                        .ok()?;
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
                    Self::search_release(&Query::new().release(album).artist(artist).build())
                        .await
                        .ok()?;
                for release in releases.releases {
                    similar_releases.push(release);
                }
            }
        }

        if let Some(album) = album {
            let releases = Self::search_release(&Query::new().release(album).build())
                .await
                .ok()?;
            for release in releases.releases {
                similar_releases.push(release);
            }
        }

        similar_releases.retain(|release| {
            let release_title_lower = release.title.to_lowercase();
            let mut keep = true;

            if let Some(album) = album {
                let album_lower = album.to_lowercase();
                if !(album_lower.contains(&release_title_lower)
                    || release_title_lower.contains(&album_lower)
                    || strsim::jaro_winkler(&album_lower, &release_title_lower) > 0.8)
                {
                    keep = false;
                }
            }

            keep
        });

        similar_releases.sort_by(|a, b| {
            let a_title = a.title.to_lowercase();
            let b_title = b.title.to_lowercase();

            let a_title_sim = strsim::jaro_winkler(&a_title, &album.unwrap_or_default());
            let b_title_sim = strsim::jaro_winkler(&b_title, &album.unwrap_or_default());

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

        let release_id = match similar_releases.first() {
            Some(release) => release.id.clone(),
            None => {
                println!("no release found");
                return None;
            }
        };

        let cover_url = format!(
            "https://coverartarchive.org/release/{}/front-250",
            release_id
        );

        let cover_res = REQWEST_CLIENT.get(&cover_url).send().await.ok()?;

        if cover_res.error_for_status().err().is_some() {
            log::info!("coverartarchive.org returned an error for {}", cover_url);
            return None;
        }

        self.cache.set(&cache_key, &cover_url.clone());

        Some(cover_url)
    }

    pub async fn search_recording(query: &str) -> Result<RecordingReponse, reqwest::Error> {
        let url = format!(
            "https://musicbrainz.org/ws/2/recording/?query={}&fmt=json&limit=5",
            query
        );
        let res = REQWEST_CLIENT.get(&url).send().await?;
        let response = res.json::<RecordingReponse>().await?;
        Ok(response)
    }
    pub async fn search_release(query: &str) -> Result<ReleaseResponse, reqwest::Error> {
        let url = format!(
            "https://musicbrainz.org/ws/2/release/?query={}&fmt=json&limit=5",
            query
        );
        let res = REQWEST_CLIENT.get(&url).send().await?;
        let response = res.json::<ReleaseResponse>().await?;
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
pub struct ReleaseResponse {
    pub created: String,
    pub count: u32,
    pub offset: u32,
    pub releases: Vec<Release>,
}

#[derive(Debug, Deserialize)]
pub struct RecordingReponse {
    pub created: String,
    pub count: u32,
    pub offset: u32,
    pub recordings: Vec<Recording>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Recording {
    pub id: String,
    pub title: String,
    #[serde(rename = "artist-credit", default)]
    pub artist_credit: Vec<ArtistCredit>,
    pub releases: Vec<Release>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ArtistCredit {
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Release {
    pub id: String,
    pub title: String,
    #[serde(rename = "artist-credit", default)]
    pub artist_credit: Vec<ArtistCredit>,
}
