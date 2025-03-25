use clap::{Parser, Subcommand};
use log::info;
use mpris::PlayerFinder;
use crate::{error::Error, config::{get_config, schema::Config}, utils::normalize_player_identity};
use std::{path::PathBuf, env};
use toml;

#[derive(Parser)]
#[command(name = "mprisence")]
#[command(about = "Discord Rich Presence for MPRIS-compatible media players")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Command {
    Players {
        #[arg(short, long)]
        detailed: bool,
    },
    
    Config,

    Version,
}

impl Command {
    pub async fn execute(self) -> Result<(), Error> {
        match self {
            Command::Players { detailed } => {
                info!("Scanning for MPRIS-compatible media players...");
                let finder = PlayerFinder::new()?;
                let players = finder.find_all()?;
                
                if players.is_empty() {
                    println!("No MPRIS-compatible media players found.");
                    return Ok(());
                }
                
                println!("\nAvailable media players:");
                println!("------------------------");
                for player in players {
                    let identity = normalize_player_identity(&player.identity());
                    let status = player.get_playback_status()
                        .map(|s| format!("{:?}", s))
                        .unwrap_or_else(|_| "Unknown".to_string());
                    
                    println!("\nPlayer: {}", identity);
                    println!("Status: {}", status);
                    
                    if detailed {
                        if let Ok(metadata) = player.get_metadata() {
                            if let Some(title) = metadata.title() {
                                println!("Title: {}", title);
                            }
                            if let Some(artists) = metadata.artists() {
                                println!("Artists: {}", artists.join(", "));
                            }
                            if let Some(album) = metadata.album_name() {
                                println!("Album: {}", album);
                            }
                            if let Some(length) = metadata.length() {
                                let duration = std::time::Duration::from_micros(length.as_micros() as u64);
                                println!("Length: {:02}:{:02}", duration.as_secs() / 60, duration.as_secs() % 60);
                            }
                        }
                        
                        // Show config for this player if it exists
                        let config = get_config();
                        let player_config = config.get_player_config(&identity);
                        println!("\nConfiguration:");
                        println!("  App ID: {}", player_config.app_id);
                        println!("  Show Icon: {}", player_config.show_icon);
                        if let Some(activity_type) = player_config.override_activity_type {
                            println!("  Activity Type: {:?}", activity_type);
                        }
                        println!("  Allow Streaming: {}", player_config.allow_streaming);
                        println!("  Ignored: {}", player_config.ignore);
                    }
                }
            }
            
            Command::Config=> {
                let config = get_config();
                println!("\nCurrent Configuration:");
                println!("---------------------");
                println!("Config file: {}", config.config_path().display());
                println!("Update interval: {}ms", config.interval());
                println!("Clear on pause: {}", config.clear_on_pause());
                
                let activity_type_config = config.activity_type_config();
                println!("\nActivity Type Settings:");
                println!("  Default type: {:?}", activity_type_config.default);
                println!("  Use content type: {}", activity_type_config.use_content_type);
                
                let time_config = config.time_config();
                println!("\nTime Display Settings:");
                println!("  Show time: {}", time_config.show);
                println!("  Show as elapsed: {}", time_config.as_elapsed);
                
                let player_configs = config.player_configs();
                if !player_configs.is_empty() {
                    println!("\nConfigured Players:");
                    for (identity, cfg) in player_configs {
                        if identity == "default" {
                            println!("\nDefault Player Configuration:");
                        } else {
                            println!("\nPlayer: {}", identity);
                        }
                        println!("  App ID: {}", cfg.app_id);
                        println!("  Show Icon: {}", cfg.show_icon);
                        if let Some(activity_type) = cfg.override_activity_type {
                            println!("  Activity Type: {:?}", activity_type);
                        }
                        println!("  Allow Streaming: {}", cfg.allow_streaming);
                        println!("  Ignored: {}", cfg.ignore);
                    }
                }
            }

            Command::Version => {
                println!("{}", env!("CARGO_PKG_VERSION"));
            }
        }
        Ok(())
    }
}
