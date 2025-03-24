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
}

impl Command {
    pub async fn execute(self) -> Result<(), Error> {
        match self {
            Command::Status => {
                println!("TODO");
            }
        }
        Ok(())
    }
}
