//! Core library for Vapourfly — Steam library manager.
//!
//! This crate contains all business logic: Steam file parsing, collection
//! management, junk detection, recommendation scoring, and playlist evaluation.
//! It has no network dependencies and no UI dependencies.

pub mod actions;
pub mod config;
pub mod discover;
pub mod display;
pub mod disposition;
pub mod dynamic;
pub mod eligibility;
pub mod error;
pub mod junk;
pub mod models;
pub mod mood;
pub mod playlist;
pub mod playlist_store;
pub mod recommend;
pub mod scoring;
pub mod share_code;
pub mod signal;
pub mod steam;
pub mod write;

pub use error::{Result, SafePath, VapourflyError};
