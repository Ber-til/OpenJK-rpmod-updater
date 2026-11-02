//! OpenJK-rpmod Updater: installs OpenJK-rpmod into Jedi Academy, and keeps it up to date.
//!
//! Opened, it looks for Jedi Academy on this computer and asks GitHub for the newest release.
//! - OpenJK-rpmod already there: up to date, or the changes since and an Update button
//! - not there: Install, into the one Jedi Academy found, or the one picked when there are more
//!
//! `--install <GameData>` goes straight to that folder (how it restarts as administrator).

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod github;
mod install;
mod installs;
mod version;

use eframe::egui;
use egui::{Color32, RichText};
use github::Release;
use installs::Install;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const ACCENT: Color32 = Color32::from_rgb(242, 190, 60);
const GOOD: Color32 = Color32::from_rgb(110, 200, 120);
const BAD: Color32 = Color32::from_rgb(235, 100, 90);

enum Stage {
    /// asking GitHub and looking for the game
    Checking,
    /// GitHub couldn't be asked
    Offline(String),
    Ready,
    Working { what: String, fraction: f32, detail: String },
    Done { message: String, first: bool },
    Failed { message: String, admin: Option<PathBuf> },
}

struct Shared {
    stage: Stage,
    releases: Vec<Release>,
    installs: Vec<Install>,
}

struct App {
    shared: Arc<Mutex<Shared>>,
    selected: usize,
    /// --install: only this folder
    forced: Option<PathBuf>,
    /// a folder picked that isn't Jedi Academy's
    pick_error: Option<String>,
    notes_cache: egui_commonmark::CommonMarkCache,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, forced: Option<PathBuf>) -> App {
        style(&cc.egui_ctx);
        let app = App {
            shared: Arc::new(Mutex::new(Shared { stage: Stage::Checking, releases: Vec::new(), installs: Vec::new() })),
            selected: 0,
            forced,
            pick_error: None,
            notes_cache: Default::default(),
        };
        app.check(&cc.egui_ctx);
        app
    }

    /// GitHub asked, and the game looked for, out of the way of the window
    fn check(&self, ctx: &egui::Context) {
        self.shared.lock().unwrap().stage = Stage::Checking;
        let shared = self.shared.clone();
        let forced = self.forced.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let found = match &forced {
                Some(path) => vec![Install::read(path)],
                None => installs::find(),
            };
            let releases = github::releases();
            let mut s = shared.lock().unwrap();
            s.installs = found;
            match releases {
                Ok(r) if !r.is_empty() => {
                    s.releases = r;
                    s.stage = Stage::Ready;
                }
                Ok(_) => s.stage = Stage::Offline("GitHub has no OpenJK-rpmod release yet".into()),
                Err(e) => s.stage = Stage::Offline(e),
            }
            ctx.request_repaint();
        });
    }

    fn start(&self, ctx: &egui::Context, game_data: PathBuf) {
        if install::game_running() {
            self.shared.lock().unwrap().stage = Stage::Failed {
                message: "Jedi Academy is running. Close it, then try again.".into(),
                admin: None,
            };
            return;
        }
        let (release, first) = {
            let s = self.shared.lock().unwrap();
            let first = s.installs.iter().find(|i| i.game_data == game_data).is_none_or(|i| i.version.is_none());
            (s.releases[0].clone(), first)
        };
        let shared = self.shared.clone();
        let ctx = ctx.clone();
        let what = if first { "Installing" } else { "Updating to" };
        shared.lock().unwrap().stage = Stage::Working {
            what: format!("{what} OpenJK-rpmod {}", release.version),
            fraction: 0.0,
            detail: "Starting...".into(),
        };

        std::thread::spawn(move || {
            let result = install::install(&release, &game_data, first, |step| {
                let (fraction, detail) = match step {
                    install::Step::Downloading { done, total } => (
                        0.85 * done as f32 / total.max(1) as f32,
                        format!("Downloading... {:.1} of {:.1} MB", done as f64 / 1e6, total as f64 / 1e6),
                    ),
                    install::Step::Unpacking { done, total } => {
                        (0.85 + 0.15 * done as f32 / total.max(1) as f32, format!("Putting the files in place... {done} of {total}"))
                    }
                };
                if let Stage::Working { fraction: f, detail: d, .. } = &mut shared.lock().unwrap().stage {
                    *f = fraction;
                    *d = detail;
                }
                ctx.request_repaint();
            });

            let mut s = shared.lock().unwrap();
            s.stage = match result {
                Ok(()) => {
                    let now = Install::read(&game_data);
                    let version = now.version.as_ref().map(|v| v.to_string()).unwrap_or_else(|| release.version.to_string());
                    if let Some(i) = s.installs.iter_mut().find(|i| i.game_data == game_data) {
                        *i = now;
                    } else {
                        s.installs.push(now);
                    }
                    let did = if first { "installed" } else { "updated" };
                    Stage::Done {
                        message: format!("OpenJK-rpmod {version} is {did} in\n{}", game_data.display()),
                        first,
                    }
                }
                Err(install::Failure::NeedsAdmin(m)) => Stage::Failed {
                    message: format!("This folder needs administrator rights to change.\n\n{m}"),
                    admin: Some(game_data),
                },
                Err(install::Failure::Other(m)) => Stage::Failed { message: m, admin: None },
            };
            ctx.request_repaint();
        });
    }

    /// the GameData folder picked by hand
    fn pick_folder(&mut self) {
        let Some(dir) = rfd::FileDialog::new().set_title("Jedi Academy's GameData folder").pick_folder() else { return };
        let game_data = if installs::is_jka(&dir) {
            dir
        } else if installs::is_jka(&dir.join("GameData")) {
            dir.join("GameData")
        } else {
            self.pick_error = Some(format!("{} isn't Jedi Academy's GameData folder: there's no base/assets0.pk3 in it.", dir.display()));
            return;
        };
        self.pick_error = None;
        let mut s = self.shared.lock().unwrap();
        self.selected = match s.installs.iter().position(|i| i.game_data == game_data) {
            Some(at) => at,
            None => {
                s.installs.push(Install::read(&game_data));
                s.installs.len() - 1
            }
        };
    }
}

fn style(ctx: &egui::Context) {
    ctx.set_visuals(egui::Visuals::dark());
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 6.0);
        style.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.55);
        style.visuals.hyperlink_color = ACCENT;
        for (text_style, font) in style.text_styles.iter_mut() {
            font.size = match text_style {
                egui::TextStyle::Heading => 22.0,
                egui::TextStyle::Small => 12.0,
                _ => 15.0,
            };
        }
    });
}

fn primary(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(egui::Button::new(RichText::new(text).strong().color(Color32::BLACK)).fill(ACCENT)).clicked()
}

fn path_text(path: &Path) -> RichText {
    RichText::new(path.display().to_string()).monospace().weak()
}

/// what's in an install, in a few words
fn status(install: &Install, latest: &Release) -> (String, Color32) {
    match &install.version {
        None => ("OpenJK-rpmod not installed".into(), Color32::GRAY),
        Some(v) if v.is_older_than(&latest.version) => (format!("{v}, update available"), ACCENT),
        Some(v) if v.is_newer_than(&latest.version) => (format!("{v}, newer than the latest release"), GOOD),
        Some(v) => (format!("{v}, up to date"), GOOD),
    }
}

/// what to do next, worked out under the lock and done after it
enum Action {
    None,
    Retry,
    Back,
    Close,
    Install(PathBuf),
    Pick,
    Admin(PathBuf),
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut action = Action::None;

        egui::Panel::top("header").show(ui, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("OpenJK-rpmod").size(26.0).strong().color(ACCENT));
                ui.label(RichText::new("installer and updater").size(15.0).weak());
            });
            ui.add_space(6.0);
        });

        egui::Panel::bottom("footer").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let ready = matches!(self.shared.lock().unwrap().stage, Stage::Ready);
                if ready && ui.small_button("Use another folder...").clicked() {
                    action = Action::Pick;
                }
                ui.hyperlink_to(RichText::new("Releases on GitHub").small(), github::RELEASES_PAGE);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(concat!("v", env!("CARGO_PKG_VERSION"))).small().weak());
                });
            });
            ui.add_space(2.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(8.0);
            let s = self.shared.lock().unwrap();
            match &s.stage {
                Stage::Checking => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label("Looking for Jedi Academy and the newest OpenJK-rpmod...");
                    });
                }
                Stage::Offline(e) => {
                    ui.label(RichText::new("Couldn't check for OpenJK-rpmod").heading());
                    ui.label(e);
                    ui.add_space(8.0);
                    if primary(ui, "Try again") {
                        action = Action::Retry;
                    }
                }
                Stage::Working { what, fraction, detail } => {
                    ui.label(RichText::new(what).heading());
                    ui.add_space(8.0);
                    ui.add(egui::ProgressBar::new(*fraction).show_percentage().fill(ACCENT.gamma_multiply(0.8)));
                    ui.label(RichText::new(detail).weak());
                }
                Stage::Done { message, first } => {
                    ui.label(RichText::new("Done").heading().color(GOOD));
                    ui.label(message);
                    if *first && cfg!(windows) {
                        ui.add_space(4.0);
                        ui.label("If you haven't used OpenJK before, tick Use OpenJK if available in RPMod-Launcher.");
                    }
                    ui.add_space(8.0);
                    if primary(ui, "Close") {
                        action = Action::Close;
                    }
                }
                Stage::Failed { message, admin } => {
                    ui.label(RichText::new("That didn't work").heading().color(BAD));
                    ui.label(message);
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if let Some(path) = admin {
                            if cfg!(windows) && primary(ui, "Restart as administrator") {
                                action = Action::Admin(path.clone());
                            }
                        }
                        if ui.button("Back").clicked() {
                            action = Action::Back;
                        }
                    });
                }
                Stage::Ready => {
                    let latest = &s.releases[0];
                    if s.installs.is_empty() {
                        ui.label(RichText::new("No Jedi Academy found").heading());
                        ui.label(if cfg!(windows) {
                            "There's no Jedi Academy with RPMod-Launcher in the usual places. Pick its GameData folder: the one with the base folder in it."
                        } else {
                            "There's no Jedi Academy in the usual places. Pick its GameData folder: the one with the base folder in it."
                        });
                        ui.add_space(8.0);
                        if primary(ui, "Choose the GameData folder...") {
                            action = Action::Pick;
                        }
                    } else {
                        self.selected = self.selected.min(s.installs.len() - 1);
                        if s.installs.len() > 1 {
                            ui.label("Jedi Academy is in more than one place. Pick the one to use:");
                            for (i, install) in s.installs.iter().enumerate() {
                                let (text, color) = status(install, latest);
                                ui.horizontal(|ui| {
                                    ui.radio_value(&mut self.selected, i, path_text(&install.game_data).strong());
                                    ui.label(RichText::new(text).small().color(color));
                                });
                            }
                            ui.separator();
                        }

                        let install = &s.installs[self.selected];
                        let path = install.game_data.clone();
                        let (title, from, button) = match &install.version {
                            None => (format!("Install OpenJK-rpmod {}?", latest.version), None, Some("Install")),
                            Some(v) if v.is_older_than(&latest.version) => {
                                (format!("OpenJK-rpmod {} is out", latest.version), Some(v.clone()), Some("Update"))
                            }
                            Some(v) if v.is_newer_than(&latest.version) => {
                                (format!("Your {v} is newer than the latest release, {}", latest.version), None, None)
                            }
                            Some(v) => (format!("OpenJK-rpmod {v} is up to date"), None, None),
                        };
                        ui.label(RichText::new(title).heading());
                        ui.horizontal_wrapped(|ui| {
                            match &install.version {
                                Some(v) => ui.label(format!("You have {v} in")),
                                None => ui.label("Into"),
                            };
                            ui.label(path_text(&path));
                        });

                        if let Some(button) = button {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if primary(ui, button) {
                                    action = Action::Install(path.clone());
                                }
                                if from.is_some() && ui.button("Not now").clicked() {
                                    action = Action::Close;
                                }
                            });
                            ui.add_space(6.0);
                            ui.label(RichText::new(if from.is_some() { "What's new" } else { "In this release" }).strong());
                            let notes = github::notes_since(&s.releases, from.as_ref());
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                    egui_commonmark::CommonMarkViewer::new().show(ui, &mut self.notes_cache, &notes);
                                });
                            });
                        } else {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                if primary(ui, "Close") {
                                    action = Action::Close;
                                }
                                if ui.button(format!("Reinstall {}", latest.version)).clicked() {
                                    action = Action::Install(path.clone());
                                }
                            });
                        }
                    }
                    if let Some(e) = &self.pick_error {
                        ui.colored_label(BAD, e);
                    }
                }
            }
        });

        match action {
            Action::None => {}
            Action::Retry => self.check(&ctx),
            Action::Back => self.shared.lock().unwrap().stage = Stage::Ready,
            Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::Install(path) => self.start(&ctx, path),
            Action::Pick => self.pick_folder(),
            Action::Admin(path) => match install::restart_as_admin(&path) {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => {
                    self.shared.lock().unwrap().stage = Stage::Failed { message: e, admin: None };
                }
            },
        }
    }
}

fn main() -> eframe::Result {
    let mut args = std::env::args().skip(1);
    let mut forced = None;
    while let Some(arg) = args.next() {
        if arg == "--install" {
            forced = args.next().map(|p| PathBuf::from(p.trim_matches('"')));
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenJK-rpmod Updater")
            .with_inner_size([640.0, 560.0])
            .with_min_inner_size([480.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native("OpenJK-rpmod Updater", options, Box::new(move |cc| Ok(Box::new(App::new(cc, forced)))))
}
