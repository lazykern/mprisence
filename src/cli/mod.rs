use clap::{Parser, Subcommand};

use crate::error::Error;

#[derive(Parser)]
#[command(name = "mprisence")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[arg(short, long)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Command {
    /// Show service status
    Status,

    /// Manage configuration
    Config {
        /// Config path (e.g., "player.spotify.ignore")
        path: Option<String>,

        /// Set value for the path
        #[arg(short, long)]
        set: Option<String>,
    },

    /// List and configure media players
    Players {
        /// Player identifier (e.g., "spotify")
        name: Option<String>,

        /// Set player config (e.g., "ignore=true")
        #[arg(short, long)]
        set: Option<String>,
    },
}

impl Command {
    pub async fn execute(self) -> Result<(), Error> {
        match self {
            Command::Status => {
                println!("TODO");
            }

            Command::Config { path, set } => {
                println!("TODO");
            }

            Command::Players { name, set } => {
                println!("TODO");
            }
        }
        Ok(())
    }
}
