use std::time::Duration;

use discord_rich_presence::activity;

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
        let details = details.into();
        if details.is_empty() {
            self.details = None;
        } else {
            self.details = Some(details);
        }
    }

    pub fn set_state<S>(&mut self, state: S)
    where
        S: Into<String>,
    {
        let state = state.into();
        if state.is_empty() {
            self.state = None;
        } else {
            self.state = Some(state);
        }
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
        let large_text = large_text.into();
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
        let small_text = small_text.into();
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
        let mut flag = self.details == other.details
            && self.state == other.state
            && self.large_image == other.large_image
            && self.large_text == other.large_text
            && self.small_image == other.small_image
            && self.small_text == other.small_text;

        // compare time by seconds
        if let Some(start_time) = self.start_time {
            if let Some(other_start_time) = other.start_time {
                flag = flag && start_time.as_secs() == other_start_time.as_secs();
            }
        }

        if let Some(end_time) = self.end_time {
            if let Some(other_end_time) = other.end_time {
                flag = flag && end_time.as_secs() == other_end_time.as_secs();
            }
        }

        flag
    }
}
