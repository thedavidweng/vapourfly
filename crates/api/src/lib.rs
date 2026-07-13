//! Steam Web API client for Vapourfly.
//!
//! This crate provides HTTP client infrastructure, disk caching, and
//! concrete API client implementations for Steam Store, IGDB, RAWG,
//! ProtonDB, PCGamingWiki, and HLTB.

pub mod cache;
pub mod enrichment;
pub mod hltb;
pub mod http;
pub mod igdb;
pub mod pcgw;
pub mod protondb;
pub mod rawg;
pub mod steam_store;
pub mod workflow;
