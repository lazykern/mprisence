use clap::Parser;
use log::{debug, error, info, trace, warn};
use parking_lot::Mutex as ParkingLotMutex;
use smallvec::SmallVec;
use std::{
    collections::{hash_map::Entry, BTreeMap, HashMap, VecDeque},
    fmt::Display,
    sync::Arc,
    thread::sleep,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use discord_presence::Client as DiscordClient;
use handlebars::Handlebars;
use mpris::{Metadata, PlaybackStatus, Player, PlayerFinder};
use smol_str::SmolStr;
use tokio::sync::{mpsc, Mutex as TokioMutex};

mod cli;
mod config;
mod cover;
mod error;
mod memory_logger;
mod utils;

use std::alloc::System;

#[global_allocator]
static GLOBAL: System = System;

mod player;
mod presence;
mod template;
mod event;
mod service;

use crate::{cli::Cli, service::Service};

#[tokio::main]
async fn main() -> Result<(), error::Error> {
    memory_logger::MemoryLogger::init().expect("Failed to initialize memory logger");

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
