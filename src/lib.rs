// #[cfg(not(target_os = "linux"))]
// compile_error!("mprisence only supports Linux systems as it relies on MPRIS (Media Player Remote Interfacing Specification)");

// Re-export modules needed for testing and external usage
pub mod config;
pub mod cover;
pub mod discord;
pub mod error;
pub mod metadata;
pub mod player;
pub mod presence;
pub mod template;
pub mod utils;
