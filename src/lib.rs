//! Original M1W protocol, reconstructed from the official macOS v500.3.3 host code.
//! Packet compatibility is not a substitute for testing against the keyboard.
pub mod access;
pub mod config;
pub mod device;
pub mod dongle;
pub mod firmware;
pub mod keys;
pub mod protocol;
pub mod transaction;
pub mod transport;
