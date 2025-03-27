use clap::{Parser, Subcommand};
use mpris::PlayerFinder;
use crate::{config::get_config, error::Error, utils::normalize_player_identity};
use std::env;

#[derive(Parser)]
#[command(name = "mprisence")]
#[command(about = "Discord Rich Presence for MPRIS media players")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    Players {
        #[command(subcommand)]
        command: PlayersCommand,
    },
    Config,
    Version {
        #[command(subcommand)]
        command: Option<VersionCommand>,
    },
}

#[derive(Subcommand)]
pub enum PlayersCommand {
    /// List available MPRIS media players
    List {
        /// Show detailed information including metadata and configuration
        #[arg(short, long)]
        detailed: bool,
    },
}

#[derive(Subcommand)]
pub enum VersionCommand {
    /// Validate a version string according to SemVer
    Validate {
        /// Version string to validate
        version: String,
    },
}

impl Command {
    pub async fn execute(self) -> Result<(), Error> {
        match self {
            Command::Players { command } => {
                match command {
                    PlayersCommand::List { detailed } => {
                        let finder = PlayerFinder::new()?;
                        let players = finder.find_all()?;
                        
                        if players.is_empty() {
                            println!("No MPRIS players found");
                            return Ok(());
                        }

                        println!("Found {} MPRIS player(s):", players.len());
                        for player in players {
                            let identity = player.identity();
                            let normalized = normalize_player_identity(&identity);
                            let status = player.get_playback_status()
                                .map(|s| format!("{:?}", s))
                                .unwrap_or_else(|_| "Unknown".to_string());

                            println!("\n{}", identity);
                            println!("  Status: {}", status);
                            
                            if detailed {
                                if let Ok(metadata) = player.get_metadata() {
                                    println!("  Metadata:");
                                    if let Some(title) = metadata.title() {
                                        println!("    Title: {}", title);
                                    }
                                    if let Some(artists) = metadata.artists() {
                                        println!("    Artists: {}", artists.join(", "));
                                    }
                                    if let Some(album) = metadata.album_name() {
                                        println!("    Album: {}", album);
                                    }
                                    if let Some(length) = metadata.length() {
                                        let duration = std::time::Duration::from_micros(length.as_micros() as u64);
                                        println!("    Length: {:02}:{:02}", duration.as_secs() / 60, duration.as_secs() % 60);
                                    }
                                }

                                let config = get_config().get_player_config(&normalized);
                                println!("  Configuration:");
                                println!("    App ID: {}", config.app_id);
                                println!("    Icon: {}", config.icon);
                                println!("    Show Icon: {}", config.show_icon);
                                println!("    Allow Streaming: {}", config.allow_streaming);
                                if let Some(activity_type) = config.override_activity_type {
                                    println!("    Activity Type: {:?}", activity_type);
                                }
                            }
                        }
                    }
                }
            }
            Command::Config => {
                let config = get_config();
                
                println!("\nGeneral Settings:");
                println!("  Config Path: {}", config.config_path().display());
                println!("  Update Interval: {}ms", config.interval());
                println!("  Clear on Pause: {}", config.clear_on_pause());

                let activity_config = config.activity_type_config();
                println!("\nActivity Settings:");
                println!("  Default Type: {:?}", activity_config.default);
                println!("  Use Content Type: {}", activity_config.use_content_type);

                let time_config = config.time_config();
                println!("\nTime Display:");
                println!("  Show Time: {}", time_config.show);
                println!("  Show as Elapsed: {}", time_config.as_elapsed);

                let cover_config = config.cover_config();
                println!("\nCover Art:");
                println!("  Providers: {:?}", cover_config.provider.provider);
                println!("  ImgBB:");
                println!("    Expiration: {}s", cover_config.provider.imgbb.expiration);

                let player_configs = config.player_configs();
                if !player_configs.is_empty() {
                    println!("\nPlayer Configurations:");
                    for (identity, cfg) in player_configs {
                        if identity == "default" {
                            println!("\n  Default Configuration:");
                        } else {
                            println!("\n  Player: {}", identity);
                        }
                        println!("    App ID: {}", cfg.app_id);
                        println!("    Icon: {}", cfg.icon);
                        println!("    Show Icon: {}", cfg.show_icon);
                        println!("    Allow Streaming: {}", cfg.allow_streaming);
                        if let Some(activity_type) = cfg.override_activity_type {
                            println!("    Activity Type: {:?}", activity_type);
                        }
                    }
                }
            }
            Command::Version { command } => {
                match command {
                    Some(VersionCommand::Validate { version }) => {
                        match crate::utils::validate_version(&version) {
                            Ok(normalized) => {
                                println!("Version '{}' is valid", normalized);
                                println!("Normalized: {}", normalized);
                            }
                            Err(e) => {
                                eprintln!("Error: {}", e);
                                std::process::exit(1);
                            }
                        }
                    }
                    None => {
                        println!("mprisence {}", env!("CARGO_PKG_VERSION"));
                    }
                }
            }
        }
        Ok(())
    }
}
