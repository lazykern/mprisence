use clap::Parser;
use log::info;

mod cli;
mod config;
mod cover;
mod error;
mod presence;
mod utils;

use std::alloc::System;

#[global_allocator]
static GLOBAL: System = System;

mod player;
mod service;
mod template;

use crate::{cli::Cli, service::Service};

#[tokio::main]
async fn main() -> Result<(), error::Error> {
    env_logger::init();

    config::initialize()?;

    let cli = Cli::parse();
    if cli.verbose {
        info!("MPRISENCE - Verbose mode enabled");
    } else {
        info!("MPRISENCE");
    }

    match cli.command {
        Some(cmd) => cmd.execute().await?,
        None => {
            let mut service = Service::new()?;
            service.run().await?;
        }
    }

    Ok(())
}
