//! Vapourfly desktop GUI: domain-facing state plus a GPUI presentation.

pub mod app;
pub mod jobs;
pub mod theme;
pub mod ui;

pub use theme::{ThemeMode, Tokens, t};

#[cfg(test)]
mod stack_contract {
    #[test]
    fn entry_bootstraps_gpui_component_init_and_root() {
        let src = include_str!("main.rs");
        assert!(
            src.contains("gpui_component::init(cx)"),
            "entry must call gpui_component::init before opening windows"
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
    fn library_and_junk_use_virtualized_uniform_list() {
        let library = include_str!("ui/library.rs");
        assert!(
            library.contains("uniform_list("),
            "library-scale collections must use gpui uniform_list"
        );
        assert!(library.contains("\"library-rows\""), "library list id");
        let junk = include_str!("ui/junk.rs");
        assert!(junk.contains("\"junk-rows\""), "junk table id");
        assert!(
            junk.contains("uniform_list("),
            "junk table must stay virtualized"
        );
    }

    #[test]
    fn jobs_repaint_through_job_wake_and_poll_resets() {
        let root = include_str!("ui/root.rs");
        assert!(
            root.contains("JobWake::new()"),
            "RepaintHook must signal JobWake so JobSlots wake the entity"
        );
        assert!(
            root.contains("this.poll_armed = false"),
            "idle poll loop must reset poll_armed so later jobs re-arm"
        );
        assert!(root.contains("playlist_name_input"));
        assert!(root.contains("playlist_id_input"));
        assert!(root.contains("playlist_desc_input"));
        assert!(root.contains("playlist_csv_input"));
    }

    #[test]
    fn backup_restore_keeps_dedicated_route() {
        let settings = include_str!("ui/settings.rs");
        assert!(
            settings.contains("begin_backup_restore"),
            "backup restore must not go through start_dry_run"
        );
    }
}
