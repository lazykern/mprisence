use clap::{Parser, Subcommand};
use log::info;
use mpris::PlayerFinder;
use crate::{error::Error, config::get_config, utils::normalize_player_identity};
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
                            println!("  {}", identity);
                            
                            if detailed {
                                let config = get_config().get_player_config(&normalized);
                                println!("    Config:");
                                println!("      app_id: {}", config.app_id);
                                println!("      icon: {}", config.icon);
                                println!("      show_icon: {}", config.show_icon);
                                println!("      allow_streaming: {}", config.allow_streaming);
                                println!("      override_activity_type: {:#?}", config.override_activity_type);
                                
                                if let Ok(metadata) = player.get_metadata() {
                                    println!("    Metadata:");
                                    if let Some(title) = metadata.title() {
                                        println!("      Title: {}", title);
                                    }
                                    if let Some(artists) = metadata.artists() {
                                        println!("      Artists: {}", artists.join(", "));
                                    }
                                    if let Some(album) = metadata.album_name() {
                                        println!("      Album: {}", album);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Command::Config => {
                unimplemented!()
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
