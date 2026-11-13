//! Original M1W protocol, reconstructed from the official macOS v500.3.3 host code.
//! Packet compatibility is not a substitute for testing against the keyboard.
pub mod access;
pub mod advanced;
pub mod config;
pub mod device;
pub mod diagnostics;
pub mod dongle;
pub mod firmware;
pub mod host;
pub mod keys;
pub mod models;
pub mod pictures;
pub mod profiles;
pub mod protocol;
pub mod review;
pub mod transaction;
pub mod transport;
