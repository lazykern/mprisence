use figment::providers::{Format, Toml};
use figment::Figment;
use log::trace;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use toml;

use crate::utils::normalize_player_identity;

const CONFIG_READY_TIMEOUT: Duration = Duration::from_millis(500);
const CONFIG_READY_POLL_INTERVAL: Duration = Duration::from_millis(25);

mod error;
pub mod schema;

pub use error::ConfigError;
pub use schema::Config;

pub type ConfigChangeReceiver = broadcast::Receiver<ConfigChange>;
type ConfigChangeSender = broadcast::Sender<ConfigChange>;

#[derive(Debug, Clone)]
pub enum ConfigChange {
    Reloaded,
    Error(String),
}

pub struct ConfigManager {
    config: Arc<RwLock<Config>>,
    path: PathBuf,
    change_tx: ConfigChangeSender,
}

static CONFIG: OnceLock<Arc<ConfigManager>> = OnceLock::new();

impl ConfigManager {
    fn new(config_path: PathBuf) -> Result<Self, ConfigError> {
        let config = load_config_from_file(&config_path)?;
        let (tx, _) = broadcast::channel(16);

        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            path: config_path,
            change_tx: tx,
        })
    }

    #[allow(dead_code)]
    pub fn new_with_config(config: Config) -> Self {
        let (tx, _) = broadcast::channel(16);

        Self {
            config: Arc::new(RwLock::new(config)),
            path: PathBuf::from("/tmp/test_config.toml"), // Dummy path
            change_tx: tx,
        }
    }

    #[allow(dead_code)]
    pub fn create_with_templates(
        detail_template: &str,
        state_template: &str,
        large_text_template: &str,
        small_text_template: &str,
    ) -> Self {
        let mut default_config = Config::default();

        default_config.template.details = detail_template.into();
        default_config.template.state = state_template.into();
        default_config.template.large_text = large_text_template.into();
        default_config.template.small_text = small_text_template.into();

        Self::new_with_config(default_config)
    }

    pub fn interval(&self) -> u64 {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .interval
    }

    pub fn allowed_players(&self) -> Vec<String> {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .allowed_players
            .clone()
    }

    pub fn is_player_allowed(&self, identity: &str, player_bus_name: &str) -> bool {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .is_player_allowed(identity, player_bus_name)
    }

    pub fn clear_on_pause(&self) -> bool {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .clear_on_pause
    }

    pub fn activity_type_config(&self) -> schema::ActivityTypesConfig {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .activity_type
            .clone()
    }

    pub fn get_player_config(&self, identity: &str, player_bus_name: &str) -> schema::PlayerConfig {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .get_player_config(identity, player_bus_name)
    }

    pub fn time_config(&self) -> schema::TimeConfig {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .time
            .clone()
    }

    pub fn template_config(&self) -> schema::TemplateConfig {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .template
            .clone()
    }

    pub fn cover_config(&self) -> schema::CoverConfig {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .cover
            .clone()
    }

    pub fn player_configs(&self) -> HashMap<String, schema::PlayerConfig> {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .effective_player_configs()
    }

    pub fn config_path(&self) -> PathBuf {
        self.path.clone()
    }

    #[allow(dead_code)]
    pub fn read(&self) -> Result<impl std::ops::Deref<Target = Config> + '_, ConfigError> {
        self.config
            .read()
            .map_err(|e| ConfigError::Lock(e.to_string()))
    }

    pub fn write(&self) -> Result<impl std::ops::DerefMut<Target = Config> + '_, ConfigError> {
        self.config
            .write()
            .map_err(|e| ConfigError::Lock(e.to_string()))
    }

    // Subscribe to config changes
    pub fn subscribe(&self) -> ConfigChangeReceiver {
        self.change_tx.subscribe()
    }

    // Save config to file
    #[allow(dead_code)]
    pub fn save(&self) -> Result<(), ConfigError> {
        let config = self.read()?;
        let config_str = toml::to_string_pretty(&*config).map_err(ConfigError::Serialize)?;
        std::fs::write(&self.path, config_str).map_err(ConfigError::IO)?;
        Ok(())
    }

    // Reload config from file
    pub fn reload(&self) -> Result<(), ConfigError> {
        log::info!("Reloading configuration from {}", self.path.display());

        wait_for_config_ready(&self.path);

        // Use the same loading logic as initial load
        let new_config = load_config_from_file(&self.path)?;

        let mut config = self.write()?;
        *config = new_config;

        let _ = self.change_tx.send(ConfigChange::Reloaded);
        Ok(())
    }
}

pub fn initialize() -> Result<(), ConfigError> {
    log::info!("Initializing configuration system");
    let config_path = get_config_path()?;
    log::debug!("Config path: {:?}", config_path);

    ensure_config_exists(&config_path)?;

    let config_manager = ConfigManager::new(config_path.clone())?;
    let config_manager = Arc::new(config_manager);

    CONFIG
        .set(config_manager.clone())
        .map_err(|_| ConfigError::AlreadyInitialized)?;

    log::debug!("Setting up config file watcher");
    setup_file_watcher(config_path, config_manager)?;

    log::info!("Configuration system initialized successfully");
    Ok(())
}

pub fn get_config() -> Arc<ConfigManager> {
    CONFIG
        .get()
        .expect("Config not initialized. Call config::initialize() first")
        .clone()
}

fn setup_file_watcher(config_path: PathBuf, config: Arc<ConfigManager>) -> Result<(), ConfigError> {
    let watched_dir = config_path.parent().unwrap().to_path_buf(); // Get parent dir
    let config_filename = config_path.file_name().map(|f| f.to_os_string()); // Get filename

    if config_filename.is_none() {
        log::error!(
            "Could not extract filename from config path: {}",
            config_path.display()
        );
        // Handle error appropriately, maybe return Err or panic depending on requirements
        return Err(ConfigError::IO(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Invalid config path",
        )));
    }
    let config_filename = config_filename.unwrap(); // Safe due to check above

    let mut last_reload = Instant::now();
    const DEBOUNCE_DURATION: Duration = Duration::from_millis(250);

    std::thread::spawn(move || {
        // Need to clone necessary items for the move closure
        let config_manager_clone = config.clone();
        let change_tx_clone = config.change_tx.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        trace!(
                            "File watcher event received: Kind={:?}, Paths={:?}",
                            event.kind,
                            event.paths
                        );

                        let is_relevant_event = event.paths.iter().any(|p| {
                            p.file_name()
                                .map_or(false, |name| name == config_filename.as_os_str())
                        });

                        let event_kind_matches = matches!(
                            event.kind,
                            notify::EventKind::Modify(_)
                                | notify::EventKind::Create(_)
                                | notify::EventKind::Remove(_)
                        );

                        if event_kind_matches && is_relevant_event {
                            log::debug!(
                                "Relevant file event detected for config: Kind={:?}, Paths={:?}",
                                event.kind,
                                event.paths
                            );
                            let now = Instant::now();
                            if now.duration_since(last_reload) >= DEBOUNCE_DURATION {
                                // Use the cloned Arc for reload
                                match config_manager_clone.reload() {
                                    Ok(_) => {
                                        last_reload = now; // Update timestamp on success
                                        log::debug!(
                                            "Config reloaded successfully after event: Kind={:?}",
                                            event.kind
                                        );
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "Failed to reload config after event Kind={:?}: {}",
                                            event.kind,
                                            e
                                        );
                                        let _ = change_tx_clone
                                            .send(ConfigChange::Error(e.to_string()));
                                    }
                                }
                            } else {
                                trace!(
                                    "Debounced config file change event (Kind={:?}, Paths={:?})",
                                    event.kind,
                                    event.paths
                                );
                            }
                        } else {
                            trace!(
                                "Ignoring non-relevant file event: Kind={:?}, Paths={:?}",
                                event.kind,
                                event.paths
                            );
                        }
                    }
                    Err(e) => {
                        log::error!("File watch error: {}", e);
                        // Use the cloned sender
                        let _ = change_tx_clone.send(ConfigChange::Error(e.to_string()));
                    }
                }
            },
            notify::Config::default(),
        )
        .expect("Failed to create watcher");

        // Watch the PARENT directory
        watcher
            .watch(&watched_dir, RecursiveMode::NonRecursive)
            .expect("Failed to watch config directory");

        log::debug!(
            "Config file watcher thread started for directory: {:?}",
            watched_dir
        );
        std::thread::park();
        log::warn!("Config file watcher thread unparked unexpectedly!");
    });

    Ok(())
}

fn wait_for_config_ready(path: &Path) {
    if path.exists() {
        return;
    }

    let start = Instant::now();
    trace!(
        "Config file {} missing, waiting up to {:?} for it to reappear",
        path.display(),
        CONFIG_READY_TIMEOUT
    );

    while start.elapsed() < CONFIG_READY_TIMEOUT {
        std::thread::sleep(CONFIG_READY_POLL_INTERVAL);
        if path.exists() {
            trace!(
                "Config file {} detected again after {:?}",
                path.display(),
                start.elapsed()
            );
            return;
        }
    }

    trace!(
        "Config file {} still missing after waiting {:?}",
        path.display(),
        CONFIG_READY_TIMEOUT
    );
}

fn get_config_path() -> Result<PathBuf, ConfigError> {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mprisence");

    std::fs::create_dir_all(&config_dir).map_err(ConfigError::IO)?;
    Ok(config_dir.join("config.toml"))
}

fn ensure_config_exists(path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ConfigError::IO)?;
    }
    Ok(())
}

fn load_config_from_file(path: &Path) -> Result<Config, ConfigError> {
    log::info!("Loading configuration from {}", path.display());

    let default_provider = Figment::new().merge(Toml::string(include_str!(
        "../../config/config.default.toml"
    )));
    let bundled: Config = default_provider
        .clone()
        .extract()
        .map_err(ConfigError::Figment)?;
    let mut figment = default_provider;
    let mut legacy_template_detail_override = None;

    if path.exists() {
        warn_deprecated_template_config(path);
        legacy_template_detail_override = read_legacy_template_detail_override(path);
        log::debug!("Merging user config from {}", path.display());
        let user_config = Figment::new().merge(Toml::file(path));
        figment = figment.merge(user_config);
    }

    let mut config: Config = figment.extract().map_err(ConfigError::Figment)?;
    if let Some(legacy_details) = legacy_template_detail_override {
        config.template.details = legacy_details;
    }
    config.bundled_player = bundled.player;
    config.user_player = load_user_player_configs(path)?;
    config.user_player_patterns = collect_user_player_patterns(path)?;
    Ok(config)
}

fn read_legacy_template_detail_override(path: &Path) -> Option<Box<str>> {
    let contents = std::fs::read_to_string(path).ok()?;
    let parsed: toml::Value = toml::from_str(&contents).ok()?;
    let template_table = parsed.get("template")?.as_table()?;

    let has_details = template_table.contains_key("details");
    if has_details {
        return None;
    }

    template_table
        .get("detail")
        .and_then(|value| value.as_str())
        .map(|value| value.to_owned().into_boxed_str())
}

fn warn_deprecated_template_config(path: &Path) {
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(_) => return,
    };

    let parsed: toml::Value = match toml::from_str(&contents) {
        Ok(value) => value,
        Err(_) => return,
    };

    let Some(template_table) = parsed.get("template").and_then(|v| v.as_table()) else {
        return;
    };

    let has_detail = template_table.contains_key("detail");
    let has_details = template_table.contains_key("details");

    if has_detail && has_details {
        log::warn!(
            "Both [template].detail (deprecated) and [template].details are set. Using [template].details."
        );
    } else if has_detail {
        log::warn!(
            "[template].detail is deprecated and will be removed in a future release. Use [template].details instead."
        );
    }
}

fn load_user_player_configs(
    path: &Path,
) -> Result<HashMap<String, schema::PlayerConfigLayer>, ConfigError> {
    if !path.exists() {
        return Ok(HashMap::new());
    }

    #[derive(serde::Deserialize)]
    struct PlayerOnly {
        #[serde(default)]
        #[serde(with = "schema::normalized_string")]
        player: HashMap<String, schema::PlayerConfigLayer>,
    }

    let contents = std::fs::read_to_string(path)?;
    let parsed: PlayerOnly = toml::from_str(&contents)?;
    Ok(parsed.player)
}

fn collect_user_player_patterns(path: &Path) -> Result<HashSet<String>, ConfigError> {
    if !path.exists() {
        return Ok(HashSet::new());
    }

    let contents = std::fs::read_to_string(path)?;
    let parsed: toml::Value = toml::from_str(&contents)?;
    let mut patterns = HashSet::new();

    if let Some(player_table) = parsed.get("player").and_then(|v| v.as_table()) {
        for key in player_table.keys() {
            patterns.insert(normalize_player_identity(key));
        }
    }

    Ok(patterns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn temp_config_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "mprisence-config-test-{}-{}",
            std::process::id(),
            unique
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("failed to create temp config dir");
        dir
    }

    #[test]
    fn reload_waits_for_config_file_to_reappear() {
        let temp_dir = temp_config_dir();
        let config_path = temp_dir.join("config.toml");

        fs::write(&config_path, "[player.default]\nshow_icon = false\n")
            .expect("failed to write initial config");

        let manager =
            ConfigManager::new(config_path.clone()).expect("failed to build config manager");

        fs::remove_file(&config_path).expect("failed to remove config file");

        let writer_path = config_path.clone();
        let writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            fs::write(&writer_path, "[player.default]\nshow_icon = true\n")
                .expect("failed to write updated config");
        });

        manager
            .reload()
            .expect("reload should wait for config file to reappear");
        writer.join().expect("writer thread panicked");

        assert!(
            manager.get_player_config("default", "default").show_icon,
            "reload should pick up the updated config content"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn legacy_template_detail_overrides_bundled_details() {
        let temp_dir = temp_config_dir();
        let config_path = temp_dir.join("config.toml");

        fs::write(&config_path, "[template]\ndetail = \"legacy override\"\n")
            .expect("failed to write config");

        let config = load_config_from_file(&config_path).expect("config should load");
        assert_eq!(config.template.details.as_ref(), "legacy override");

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn template_details_wins_when_both_keys_exist_in_user_config() {
        let temp_dir = temp_config_dir();
        let config_path = temp_dir.join("config.toml");

        fs::write(
            &config_path,
            "[template]\ndetail = \"legacy override\"\ndetails = \"new override\"\n",
        )
        .expect("failed to write config");

        let config = load_config_from_file(&config_path).expect("config should load");
        assert_eq!(config.template.details.as_ref(), "new override");

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
