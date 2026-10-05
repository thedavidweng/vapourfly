//! Vapourfly desktop GUI: domain-facing state plus a GPUI presentation.

pub mod app;
mod artwork;
pub mod gamepad;
pub mod jobs;
pub mod platform;
pub mod theme;
pub mod ui;

pub use theme::{ThemeMode, Tokens, t};

#[cfg(test)]
mod stack_contract {
    #[test]
    fn entry_bootstraps_gpui_component_init_and_root() {
        let src = include_str!("main.rs");
        assert!(
            src.contains("gpui_kit::init(cx)"),
            "entry must call gpui_kit::init before opening windows"
        );
        assert!(
            src.contains("Root::new(view, window, cx)"),
            "the first window view must be wrapped in Root"
        );
        let banned = format!("{}{}", "ef", "rame::");
        assert!(
            !src.contains(&banned),
            "entry must not start that immediate-mode toolkit app"
        );
    }

    #[test]
    fn library_and_junk_use_virtualized_lists() {
        let src = include_str!("ui/library.rs");
        assert!(
            src.contains("uniform_list("),
            "library-scale rows must use gpui uniform_list"
        );
        assert!(
            src.contains("list(\n            self.library_scroll.clone()"),
            "the library grid feed must use the virtualized gpui list"
        );
        assert!(src.contains("\"library-rows\""), "library list id");
        assert!(src.contains("\"junk-rows\""), "junk table id");
    }

    #[test]
    fn jobs_repaint_through_job_wake_and_poll_resets() {
        let src = concat!(include_str!("ui/mod.rs"), include_str!("ui/manage.rs"));
        assert!(
            src.contains("JobWake::new()"),
            "RepaintHook must signal JobWake so JobSlots wake the entity"
        );
        assert!(
            src.contains("this.poll_armed = false"),
            "idle poll loop must reset poll_armed so later jobs re-arm"
        );
        assert!(
            src.contains("begin_backup_restore"),
            "backup restore must not go through start_dry_run"
        );
        assert!(src.contains("playlist_name_input"));
        assert!(src.contains("playlist_id_input"));
        assert!(src.contains("playlist_desc_input"));
        assert!(src.contains("playlist_csv_input"));
    }
}
