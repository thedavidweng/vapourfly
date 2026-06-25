use std::path::PathBuf;

use eframe::egui;
use vapourfly_core::models::ScanResult;
use vapourfly_core::steam::scan::{ScanOptions, scan_library};

// ---------------------------------------------------------------------------
// View enum
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Library,
    Junk,
    Recommend,
    Playlists,
    Collections,
    DataSources,
    Backups,
    Settings,
}

impl View {
    const ALL: &'static [View] = &[
        View::Library,
        View::Junk,
        View::Recommend,
        View::Playlists,
        View::Collections,
        View::DataSources,
        View::Backups,
        View::Settings,
    ];

    fn label(&self) -> &'static str {
        match self {
            View::Library => "Library",
            View::Junk => "Junk",
            View::Recommend => "Recommend",
            View::Playlists => "Playlists",
            View::Collections => "Collections",
            View::DataSources => "Data Sources",
            View::Backups => "Backups",
            View::Settings => "Settings",
        }
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct VapourflyApp {
    scan_result: Option<ScanResult>,
    current_view: View,
    loading: bool,
    error: Option<String>,
    fixtures_path: Option<PathBuf>,
}

impl VapourflyApp {
    fn new(fixtures_path: Option<PathBuf>) -> Self {
        Self {
            scan_result: None,
            current_view: View::Library,
            loading: false,
            error: None,
            fixtures_path,
        }
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;

        let ctx = ctx.clone();
        let fixtures = self.fixtures_path.clone();

        std::thread::spawn(move || {
            let opts = ScanOptions {
                steam_dir: dirs::home_dir()
                    .unwrap_or_default()
                    .join(".steam")
                    .join("steam"),
                account: None,
                fixtures,
            };

            let result = scan_library(&opts);

            // Wake the UI thread on completion.
            ctx.request_repaint();

            // We communicate the result back via a global-ish channel.
            // For simplicity we use a static mutex; a real app might use
            // an Arc<Mutex<>> inside the app struct instead.
            SCAN_RESULT.lock().unwrap().replace(result);
        });
    }
}

use std::sync::Mutex;

static SCAN_RESULT: Mutex<Option<vapourfly_core::Result<ScanResult>>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// eframe::App implementation
// ---------------------------------------------------------------------------

impl eframe::App for VapourflyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll background scan result.
        if self.loading {
            let mut guard = SCAN_RESULT.lock().unwrap();
            if let Some(result) = guard.take() {
                self.loading = false;
                match result {
                    Ok(scan) => self.scan_result = Some(scan),
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }

        // -- Left panel: navigation -----------------------------------------
        egui::SidePanel::left("nav_panel")
            .resizable(false)
            .default_width(180.0)
            .show(ctx, |ui| {
                ui.heading("Vapourfly");
                ui.separator();

                for &view in View::ALL {
                    let selected = self.current_view == view;
                    if ui.selectable_label(selected, view.label()).clicked() {
                        self.current_view = view;
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.separator();
                    if ui.button("Refresh").clicked() {
                        self.start_scan(ctx);
                    }
                });
            });

        // -- Central panel: current view ------------------------------------
        egui::CentralPanel::default().show(ctx, |ui| {
            // Loading indicator.
            if self.loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Scanning Steam library...");
                });
                ui.separator();
            }

            // Error display.
            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::RED, format!("Error: {err}"));
                ui.separator();
            }

            // View content.
            match self.current_view {
                View::Library => render_library(ui, &self.scan_result),
                View::Junk => render_placeholder(ui, "Junk detection"),
                View::Recommend => render_placeholder(ui, "Recommendations"),
                View::Playlists => render_placeholder(ui, "Playlists"),
                View::Collections => render_placeholder(ui, "Collections"),
                View::DataSources => render_placeholder(ui, "Data Sources"),
                View::Backups => render_placeholder(ui, "Backups"),
                View::Settings => render_placeholder(ui, "Settings"),
            }
        });

        // Kick off initial scan on first frame.
        if self.scan_result.is_none() && !self.loading && self.error.is_none() {
            self.start_scan(ctx);
        }
    }
}

// ---------------------------------------------------------------------------
// View renderers
// ---------------------------------------------------------------------------

fn render_library(ui: &mut egui::Ui, scan_result: &Option<ScanResult>) {
    match scan_result {
        None => {
            ui.label("No scan data yet. Waiting for scan to complete...");
        }
        Some(scan) => {
            ui.heading(format!(
                "Library \u{2014} {} ({})",
                scan.account, scan.steam_dir
            ));
            ui.separator();

            let text_height = egui::TextStyle::Body.resolve(ui.style()).size;

            egui_extras::TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(egui_extras::Column::auto().at_least(60.0)) // App ID
                .column(egui_extras::Column::remainder().at_least(200.0)) // Name
                .column(egui_extras::Column::auto().at_least(80.0)) // Installed
                .column(egui_extras::Column::auto().at_least(100.0)) // Playtime
                .header(text_height * 1.4, |mut header| {
                    header.col(|ui| {
                        ui.strong("App ID");
                    });
                    header.col(|ui| {
                        ui.strong("Name");
                    });
                    header.col(|ui| {
                        ui.strong("Installed");
                    });
                    header.col(|ui| {
                        ui.strong("Playtime");
                    });
                })
                .body(|mut body| {
                    for game in &scan.games {
                        body.row(text_height * 1.2, |mut row| {
                            row.col(|ui| {
                                ui.label(game.app_id.to_string());
                            });
                            row.col(|ui| {
                                ui.label(&game.name);
                            });
                            row.col(|ui| {
                                ui.label(if game.installed {
                                    "\u{2713}"
                                } else {
                                    "\u{2014}"
                                });
                            });
                            row.col(|ui| {
                                let pt = game.playtime_minutes.unwrap_or(0);
                                ui.label(format_playtime(pt));
                            });
                        });
                    }
                });
        }
    }
}

fn render_placeholder(ui: &mut egui::Ui, title: &str) {
    ui.heading(title);
    ui.separator();
    ui.label("Not yet implemented.");
}

fn format_playtime(minutes: u32) -> String {
    if minutes == 0 {
        return "\u{2014}".to_string();
    }
    let hours = minutes / 60;
    let mins = minutes % 60;
    if hours == 0 {
        format!("{mins}m")
    } else {
        format!("{hours}h {mins}m")
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> eframe::Result<()> {
    // Parse --fixtures flag.
    let fixtures_path = std::env::args()
        .collect::<Vec<String>>()
        .windows(2)
        .find(|w| w[0] == "--fixtures")
        .map(|w| PathBuf::from(&w[1]));

    let app = VapourflyApp::new(fixtures_path);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Vapourfly",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}
