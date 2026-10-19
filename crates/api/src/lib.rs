//! Steam Web API client for Vapourfly.
//!
//! This crate provides HTTP client infrastructure, disk caching, and
//! concrete API client implementations for Steam Store, IGDB, RAWG,
//! ProtonDB, and PCGamingWiki.

pub mod achievements;
pub mod activity;
pub mod cache;
pub mod enrichment;
pub mod http;
pub mod igdb;
pub mod pcgw;
pub mod profile;
pub mod protondb;
pub mod rawg;
pub mod steam_store;
pub mod steam_web;
pub mod workflow;
