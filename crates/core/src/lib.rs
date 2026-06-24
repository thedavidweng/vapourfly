//! Core library for Vapourfly — Steam library manager.
//!
//! This crate contains all business logic: Steam file parsing, collection
//! management, junk detection, recommendation scoring, and playlist evaluation.
//! It has no network dependencies and no UI dependencies.

pub mod config;
pub mod error;
pub mod models;

pub use error::{Result, SafePath, VapourflyError};
