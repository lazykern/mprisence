use figment::providers::{Format, Toml};
use figment::Figment;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

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

        default_config.template.detail = detail_template.into();
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

    pub fn get_player_config(&self, identity: &str) -> schema::PlayerConfig {
        self.config
            .read()
            .expect("Failed to read config: RwLock poisoned")
            .get_player_config(identity)
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
            .player
            .clone()
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
    let path_to_watch = config_path.clone();
    let mut last_reload = Instant::now();
    const DEBOUNCE_DURATION: Duration = Duration::from_millis(250);

    std::thread::spawn(move || {
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<notify::Event, notify::Error>| match res {
                Ok(event) => {
                    if (matches!(event.kind, notify::EventKind::Modify(_)) || 
                        matches!(event.kind, notify::EventKind::Remove(_)))
                        && event.paths.iter().any(|p| p == &path_to_watch)
                    {
                        let now = Instant::now();
                        if now.duration_since(last_reload) >= DEBOUNCE_DURATION {
                            last_reload = now;
                            match config.reload() {
                                Ok(_) => {
                                    if matches!(event.kind, notify::EventKind::Remove(_)) {
                                        log::info!("Config file was deleted, using default configuration");
                                    } else {
                                        log::debug!("Config reloaded successfully");
                                    }
                                },
                                Err(e) => {
                                    log::warn!("Failed to reload config: {}", e);
                                    let _ = config.change_tx.send(ConfigChange::Error(e.to_string()));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!("File watch error: {}", e);
                    let _ = config.change_tx.send(ConfigChange::Error(e.to_string()));
                }
            },
            notify::Config::default(),
        )
        .expect("Failed to create watcher");

        watcher
            .watch(config_path.parent().unwrap(), RecursiveMode::NonRecursive)
            .expect("Failed to watch config directory");

        std::thread::park();
    });

    Ok(())
}

fn get_config_path() -> Result<PathBuf, ConfigError> {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mprisence");

    std::fs::create_dir_all(&config_dir).map_err(ConfigError::IO)?;
    Ok(config_dir.join("config.toml"))
}

fn ensure_config_exists(path: &Path) -> Result<(), ConfigError> {
    // Only ensure parent directory exists, don't create the file
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(ConfigError::IO)?;
    }
    Ok(())
}

fn load_config_from_file(path: &Path) -> Result<Config, ConfigError> {
    log::info!("Loading configuration from {}", path.display());

    let mut figment = Figment::new().merge(Toml::string(include_str!(
        "../../config/config.default.toml"
    )));

    // Merge user config if it exists
    if path.exists() {
        log::debug!("Merging user config from {}", path.display());
        figment = figment.merge(Toml::file(path));
    }

    figment.extract().map_err(ConfigError::Figment)
}
