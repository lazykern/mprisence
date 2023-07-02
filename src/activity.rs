use std::time::{Duration, SystemTime, UNIX_EPOCH};

use discord_rich_presence::activity;

use crate::{context::Context, CONFIG};

#[derive(Debug, Clone, Default)]
pub struct Activity {
    details: Option<String>,
    state: Option<String>,
    large_image: Option<String>,
    large_text: Option<String>,
    small_image: Option<String>,
    small_text: Option<String>,
    start_time: Option<Duration>,
    end_time: Option<Duration>,
}

impl Activity {
    pub fn new() -> Self {
        Self {
            details: None,
            state: None,
            large_image: None,
            large_text: None,
            small_image: None,
            small_text: None,
            start_time: None,
            end_time: None,
        }
    }

    pub fn set_details<S>(&mut self, details: S)
    where
        S: Into<String>,
    {
        let details = details.into() + "\0";
        if details.is_empty() {
            self.details = None;
            return;
        }
        self.details = Some(details);
    }

    pub fn set_state<S>(&mut self, state: S)
    where
        S: Into<String>,
    {
        let state = state.into() + "\0";
        if state.is_empty() {
            self.state = None;
        }
        self.state = Some(state);
    }

    pub fn set_large_image<S>(&mut self, large_image: S)
    where
        S: Into<String>,
    {
        let large_image = large_image.into();
        if large_image.is_empty() {
            self.large_image = None;
        } else {
            self.large_image = Some(large_image);
        }
    }

    pub fn set_large_text<S>(&mut self, large_text: S)
    where
        S: Into<String>,
    {
        let large_text = large_text.into() + "\0";
        if large_text.is_empty() {
            self.large_text = None;
        } else {
            self.large_text = Some(large_text);
        }
    }

    pub fn set_small_image<S>(&mut self, small_image: S)
    where
        S: Into<String>,
    {
        let small_image = small_image.into();
        if small_image.is_empty() {
            self.small_image = None;
        } else {
            self.small_image = Some(small_image);
        }
    }

    pub fn set_small_text<S>(&mut self, small_text: S)
    where
        S: Into<String>,
    {
        let small_text = small_text.into() + "\0";
        if small_text.is_empty() {
            self.small_text = None;
        } else {
            self.small_text = Some(small_text);
        }
    }

    pub fn set_start_time(&mut self, start_time: Duration) {
        self.start_time = Some(start_time);
    }

    pub fn set_end_time(&mut self, end_time: Duration) {
        self.end_time = Some(end_time);
    }

    pub fn set_timestamps_from_context(&mut self, context: &Context) {
        // Get the current time.
        let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(t) => t,
            Err(e) => {
                log::error!("Error getting current time: {:?}", e);
                return;
            }
        };

        // Get the current track's position.
        let position = match context.player() {
            Some(p) => p.get_position(),
            None => {
                log::warn!("No player in context, returning timestamps as none");
                return;
            }
        };

        let position_dur = match position {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Error getting position: {:?}", e);
                return;
            }
        };
        log::debug!("Position: {:?}", position_dur);

        // Subtract the position from the current time. This will give us the amount
        // of time that has elapsed since the start of the track.
        let start_dur = match now > position_dur {
            true => now - position_dur,
            false => now,
        };
        log::debug!("Start duration: {:?}", start_dur);

        if CONFIG.time.as_elapsed {
            // Set the start timestamp.
            self.set_start_time(start_dur);
        }

        // Get the current track's metadata.
        let m = match context.metadata() {
            Some(m) => m,
            None => {
                log::warn!("No metadata in context, returning timestamps as none");
                return;
            }
        };

        // Get the current track's length.
        let length = match m.length() {
            Some(l) => l,
            None => {
                log::warn!("No length in metadata, returning timestamps as none");
                return;
            }
        };

        // Add the start time to the track length. This gives us the time that the
        // track will end at.
        let end_dur = start_dur + length;
        log::debug!("End duration: {:?}", end_dur);

        // Set the end timestamp.
        self.set_end_time(end_dur);
    }

    pub fn to_discord_activity(&self) -> activity::Activity<'_> {
        let mut discord_activiity = activity::Activity::new();

        if let Some(details) = &self.details {
            discord_activiity = discord_activiity.details(details);
        }

        if let Some(state) = &self.state {
            discord_activiity = discord_activiity.state(state);
        }

        let mut assets = activity::Assets::new();

        if let Some(large_image) = &self.large_image {
            assets = assets.large_image(large_image);

            if let Some(large_text) = &self.large_text {
                assets = assets.large_text(large_text);
            }

            if let Some(small_image) = &self.small_image {
                assets = assets.small_image(small_image);
                if let Some(small_text) = &self.small_text {
                    assets = assets.small_text(small_text);
                }
            }
            discord_activiity = discord_activiity.assets(assets);
        }

        let mut timestamps = activity::Timestamps::new();
        let mut has_timestamps = false;
        if let Some(start_time) = &self.start_time {
            timestamps = timestamps.start(start_time.as_secs() as i64);
            has_timestamps = true;
        }

        if let Some(end_time) = &self.end_time {
            timestamps = timestamps.end(end_time.as_secs() as i64);
            has_timestamps = true;
        }

        if has_timestamps {
            discord_activiity = discord_activiity.timestamps(timestamps);
        }

        discord_activiity
    }
}

impl Eq for Activity {}

impl PartialEq for Activity {
    fn eq(&self, other: &Self) -> bool {
        self.details == other.details
            && self.state == other.state
            && self.large_image == other.large_image
            && self.large_text == other.large_text
            && self.small_image == other.small_image
            && self.small_text == other.small_text
            && self.start_time.map(|t| t.as_secs()) == other.start_time.map(|t| t.as_secs())
            && self.end_time.map(|t| t.as_secs()) == other.end_time.map(|t| t.as_secs())
    }
}
