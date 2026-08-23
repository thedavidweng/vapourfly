//! GPUI + gpui-component presentation for [`crate::app::VapourflyApp`].
//!
//! Views live in per-feature modules; [`GuiRoot`] owns the application state
//! and the retained input entities. Shared presentation helpers are in
//! [`shared`], the token bridge in [`appearance`].

pub mod appearance;
pub mod chrome;
pub mod collections;
pub mod data_sources;
pub mod discover;
pub mod junk;
pub mod library;
mod overlays;
pub mod playlists;
pub mod recommend;
pub mod root;
pub mod settings;
pub mod shared;

pub use root::GuiRoot;
