//! Zakuro's setup: installs Zakuro for the current user, downloading the
//! Zig compiler that recompiling games uses, and, run as uninstall.exe or
//! with --uninstall, takes it away again, the player choosing whether their
//! games, saves and settings go too.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod system;
mod work;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, Vec2};

use work::{DataSizes, InstallPlan, Progress, Shared, UninstallPlan};

const VERSION: &str = env!("CARGO_PKG_VERSION");

mod color {
    use eframe::egui::Color32;
    pub const BASE: Color32 = Color32::from_rgb(0x12, 0x14, 0x1a);
    pub const CARD: Color32 = Color32::from_rgb(0x1a, 0x1d, 0x26);
    pub const RAISED: Color32 = Color32::from_rgb(0x24, 0x28, 0x34);
    pub const ACCENT: Color32 = Color32::from_rgb(0x38, 0xbd, 0xf8);
    pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe8, 0xee);
    pub const MUTED: Color32 = Color32::from_rgb(0x8b, 0x92, 0xa5);
    pub const DANGER: Color32 = Color32::from_rgb(0xf8, 0x71, 0x71);
    pub const WARN: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    Install,
    Uninstall,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Page {
    Choose,
    Working,
    Done,
}

struct Args {
    uninstall: bool,
    quiet: bool,
    /// the install an uninstaller copied to the temporary folder works on.
    dir: Option<PathBuf>,
    /// this program is that copy, to delete when it ends.
    relaunched: bool,
}

fn args() -> Args {
    let mut args = Args { uninstall: false, quiet: false, dir: None, relaunched: false };
    let mut list = std::env::args_os().skip(1);
    while let Some(arg) = list.next() {
        match arg.to_str() {
            Some("--uninstall") => args.uninstall = true,
            Some("--quiet") => args.quiet = true,
            Some("--relaunched") => args.relaunched = true,
            Some("--dir") => args.dir = list.next().map(PathBuf::from),
            _ => {}
        }
    }
    // uninstall.exe, as setup leaves itself, uninstalls
    if std::env::current_exe().is_ok_and(|exe| is_uninstaller(&exe)) {
        args.uninstall = true;
    }
    args
}

/// whether the program at exe is the uninstaller setup leaves.
fn is_uninstaller(exe: &std::path::Path) -> bool {
    exe.file_stem().is_some_and(|stem| stem.to_string_lossy().to_ascii_lowercase().starts_with("uninstall"))
}

fn main() {
    let mut args = args();
    if args.uninstall && !args.relaunched {
        // an uninstaller in the folder it removes can't delete itself, so it
        // runs from a copy in the temporary folder, which goes once it ends
        let me = std::env::current_exe().ok();
        // the folder it is in when it is the uninstall.exe setup left there,
        // else the one Installed apps names: a setup run with --uninstall
        // from anywhere else mustn't take its own folder for Zakuro's
        let left = me.as_ref().filter(|me| is_uninstaller(me) && me.parent().is_some_and(|dir| dir.join("zakuro.exe").is_file()));
        let dir = args.dir.clone().or_else(|| left.and_then(|me| me.parent().map(PathBuf::from))).or_else(system::installed_at);
        let copy = std::env::temp_dir().join(format!("zakuro-uninstall-{}.exe", std::process::id()));
        if let (Some(me), Some(dir)) = (&me, &dir) {
            if std::fs::copy(me, &copy).is_ok() {
                let mut command = std::process::Command::new(&copy);
                command.args(["--uninstall", "--relaunched", "--dir"]).arg(dir);
                if args.quiet {
                    command.arg("--quiet");
                }
                if command.spawn().is_ok() {
                    return;
                }
            }
        }
        args.relaunched = false;
        args.dir = dir;
    }
    // the copy in the temporary folder goes once it ends
    if args.relaunched {
        if let Ok(me) = std::env::current_exe() {
            system::delete_after_exit(&me);
        }
    }
    if args.uninstall && args.dir.is_none() {
        args.dir = system::installed_at();
    }
    if args.uninstall && args.quiet {
        // Installed apps' quiet uninstall keeps the player's data
        let progress = Shared::default();
        work::uninstall(UninstallPlan { dir: args.dir.clone(), ..UninstallPlan::default() }, progress.clone());
        while !progress.lock().is_ok_and(|progress| progress.finished) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        return;
    }

    let mode = if args.uninstall { Mode::Uninstall } else { Mode::Install };
    let title = match mode {
        Mode::Install => format!("Zakuro {VERSION} Setup"),
        Mode::Uninstall => "Uninstall Zakuro".to_owned(),
    };
    let mut viewport = egui::ViewportBuilder::default().with_title(&title).with_inner_size([600.0, 540.0]).with_resizable(false).with_maximize_button(false);
    if let Ok(image) = image::load_from_memory(include_bytes!("../../../packaging/zakuro.png")) {
        let rgba = image.to_rgba8();
        let (width, height) = rgba.dimensions();
        viewport = viewport.with_icon(Arc::new(egui::IconData { rgba: rgba.into_raw(), width, height }));
    }
    let options = eframe::NativeOptions { viewport, centered: true, ..eframe::NativeOptions::default() };
    let result = eframe::run_native(
        &title,
        options,
        Box::new(move |cc| {
            style(&cc.egui_ctx);
            Ok(Box::new(Setup::new(mode, args.dir)))
        }),
    );
    if let Err(error) = result {
        // with no window to say it in, Windows' own box says it
        let _ = rfd::MessageDialog::new().set_title(&title).set_description(format!("Setup couldn't open its window: {error}")).show();
    }
}

fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = color::BASE;
    visuals.window_fill = color::CARD;
    visuals.extreme_bg_color = color::BASE;
    visuals.override_text_color = Some(color::TEXT);
    visuals.selection.bg_fill = color::ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, color::BASE);
    for widgets in [&mut visuals.widgets.inactive, &mut visuals.widgets.hovered, &mut visuals.widgets.active, &mut visuals.widgets.noninteractive] {
        widgets.corner_radius = CornerRadius::same(4);
    }
    visuals.widgets.inactive.weak_bg_fill = color::RAISED;
    visuals.widgets.inactive.bg_fill = color::RAISED;
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x2e, 0x34, 0x44);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, color::ACCENT);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x2e, 0x34, 0x44);
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_visuals_of(egui::Theme::Dark, visuals);
    ctx.style_mut_of(egui::Theme::Dark, |style| {
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(16.0, 8.0);
        style.spacing.interact_size.y = 32.0;
    });
}

struct Setup {
    mode: Mode,
    page: Page,
    /// install: the folder, as typed.
    dir: String,
    /// install: Zakuro is there already, which this updates.
    updating: bool,
    desktop: bool,
    zig: bool,
    /// uninstall: the install's folder.
    install_dir: Option<PathBuf>,
    games: bool,
    saves: bool,
    settings: bool,
    sizes: Option<DataSizes>,
    measuring: Option<Receiver<DataSizes>>,
    progress: Shared,
    cancel: Arc<AtomicBool>,
}

impl Setup {
    fn new(mode: Mode, install_dir: Option<PathBuf>) -> Setup {
        let existing = system::installed_at();
        let dir = existing.clone().unwrap_or_else(system::default_install_dir);
        let measuring = (mode == Mode::Uninstall).then(|| {
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = send.send(work::data_sizes());
            });
            receive
        });
        Setup {
            mode,
            page: Page::Choose,
            updating: existing.as_ref().is_some_and(|dir| dir.join("zakuro.exe").is_file()),
            dir: dir.display().to_string(),
            desktop: existing.is_none(),
            zig: true,
            install_dir,
            games: false,
            saves: false,
            settings: false,
            sizes: None,
            measuring,
            progress: Shared::default(),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn start(&mut self) {
        self.progress = Shared::default();
        self.cancel = Arc::new(AtomicBool::new(false));
        self.page = Page::Working;
        match self.mode {
            Mode::Install => {
                let plan = InstallPlan { dir: PathBuf::from(self.dir.trim()), desktop: self.desktop, zig: self.zig, version: VERSION.to_owned() };
                work::install(plan, self.progress.clone(), self.cancel.clone());
            }
            Mode::Uninstall => {
                let plan = UninstallPlan { dir: self.install_dir.clone(), games: self.games, saves: self.saves, settings: self.settings };
                work::uninstall(plan, self.progress.clone());
            }
        }
    }
}

impl eframe::App for Setup {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(receive) = &self.measuring {
            if let Ok(sizes) = receive.try_recv() {
                self.sizes = Some(sizes);
                self.measuring = None;
            }
        }
        let progress = self.progress.lock().map(|progress| progress.clone()).unwrap_or_default();
        if self.page == Page::Working && progress.finished {
            self.page = Page::Done;
        }
        if self.page == Page::Working || self.measuring.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(color::BASE).inner_margin(Margin::same(28))).show(ui, |ui| {
            header(ui, self.mode);
            ui.add_space(16.0);
            let close = match (self.mode, self.page) {
                (Mode::Install, Page::Choose) => self.install_choices(ui),
                (Mode::Uninstall, Page::Choose) => self.uninstall_choices(ui),
                (_, Page::Working) => self.working(ui, &progress),
                (_, Page::Done) => self.done(ui, &progress),
            };
            if close {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

fn header(ui: &mut egui::Ui, mode: Mode) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Zakuro").size(26.0).strong().color(color::TEXT));
        ui.label(RichText::new(VERSION).size(14.0).color(color::MUTED));
    });
    let line = match mode {
        Mode::Install => "A Nintendo 3DS emulator",
        Mode::Uninstall => "Uninstall",
    };
    ui.label(RichText::new(line).color(color::MUTED));
}

/// a box grouping some choices.
fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(color::CARD).corner_radius(CornerRadius::same(12)).inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

fn note(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).size(12.5).color(color::MUTED));
}

/// the buttons at the bottom right, the first the main one. which was
/// clicked, if one was.
fn buttons(ui: &mut egui::Ui, labels: &[(&str, bool)]) -> Option<usize> {
    let mut clicked = None;
    // down at the bottom, however much is above them
    ui.add_space((ui.available_height() - 40.0).max(0.0));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        for (index, (label, enabled)) in labels.iter().enumerate().rev() {
            let text = RichText::new(*label).size(15.0);
            let button = if index == 0 { egui::Button::new(text.color(color::BASE).strong()).fill(color::ACCENT) } else { egui::Button::new(text) };
            if ui.add_enabled(*enabled, button.corner_radius(CornerRadius::same(8)).min_size(Vec2::new(110.0, 36.0))).clicked() {
                clicked = Some(index);
            }
        }
    });
    clicked
}

impl Setup {
    fn install_choices(&mut self, ui: &mut egui::Ui) -> bool {
        card(ui, |ui| {
            ui.label(RichText::new("Install to").strong());
            ui.horizontal(|ui| {
                let field = egui::TextEdit::singleline(&mut self.dir).desired_width(ui.available_width() - 100.0).margin(Margin::symmetric(8, 7));
                ui.add(field);
                if ui.button("Change…").clicked() {
                    if let Some(folder) = rfd::FileDialog::new().set_title("Where to install Zakuro").set_directory(&self.dir).pick_folder() {
                        // a folder picked for it holds Zakuro in its own folder
                        let named = folder.file_name().is_some_and(|name| name.eq_ignore_ascii_case("zakuro"));
                        self.dir = if named { folder } else { folder.join("Zakuro") }.display().to_string();
                    }
                }
            });
            if self.updating {
                note(ui, "Zakuro is installed here already, and this updates it. Your games, saves and settings stay.");
            } else {
                note(ui, "Games, saves and settings are kept apart from the program, in your user folder.");
            }
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            let zig_there = work::zig_in(&PathBuf::from(self.dir.trim())).is_file();
            ui.add_enabled_ui(!zig_there, |ui| {
                ui.checkbox(&mut self.zig, RichText::new("Set up recompiling").strong());
            });
            let text = if zig_there {
                format!("The Zig {} compiler it uses is there already.", work::ZIG_VERSION)
            } else {
                format!(
                    "Downloads the Zig {} compiler ({}) from ziglang.org. Recompiling turns a game's code into native code, so it runs faster. Without it games still play.",
                    work::ZIG_VERSION,
                    system::human(work::ZIG_SIZE)
                )
            };
            note(ui, text);
            ui.add_space(4.0);
            ui.checkbox(&mut self.desktop, "Add a desktop shortcut");
        });
        let ready = !self.dir.trim().is_empty() && work::carries_zakuro();
        if !work::carries_zakuro() {
            ui.label(RichText::new("This setup was built without Zakuro in it.").color(color::DANGER));
        }
        match buttons(ui, &[(if self.updating { "Update" } else { "Install" }, ready), ("Cancel", true)]) {
            Some(0) => self.start(),
            Some(_) => return true,
            None => {}
        }
        false
    }

    fn uninstall_choices(&mut self, ui: &mut egui::Ui) -> bool {
        card(ui, |ui| {
            ui.label(RichText::new("Zakuro and the compiler it recompiles with are removed.").strong());
            match &self.install_dir {
                Some(dir) => note(ui, format!("From {}", dir.display())),
                None => note(ui, "Its folder wasn't found, so only its shortcuts and data can go."),
            }
        });
        ui.add_space(8.0);
        let sizes = self.sizes.clone();
        let size = |pick: fn(&DataSizes) -> u64| sizes.as_ref().map_or("measuring…".to_owned(), |sizes| system::human(pick(sizes)));
        card(ui, |ui| {
            ui.label(RichText::new("Also delete").strong());
            ui.checkbox(&mut self.games, format!("My games and their recompiled code ({})", size(|s| s.games)));
            if let Some(library) = sizes.as_ref().and_then(|sizes| sizes.library.as_ref()) {
                note(ui, format!("    In {}", library.display()));
            }
            ui.checkbox(&mut self.saves, format!("My saves and save states ({})", size(|s| s.saves)));
            ui.checkbox(&mut self.settings, format!("My settings, cheats, mods and texture packs ({})", size(|s| s.settings)));
            ui.add_space(2.0);
            if self.games || self.saves || self.settings {
                ui.label(RichText::new("What is ticked can't be brought back.").size(12.5).color(color::DANGER));
            } else {
                note(ui, "What isn't ticked stays, and Zakuro finds it again if it is installed later.");
            }
        });
        match buttons(ui, &[("Uninstall", true), ("Cancel", true)]) {
            Some(0) => self.start(),
            Some(_) => return true,
            None => {}
        }
        false
    }

    fn working(&mut self, ui: &mut egui::Ui, progress: &Progress) -> bool {
        card(ui, |ui| {
            ui.label(RichText::new(if progress.step.is_empty() { "Starting" } else { &progress.step }).strong());
            ui.add_space(4.0);
            let bar = egui::ProgressBar::new(progress.fraction.unwrap_or(0.0)).desired_height(10.0).fill(color::ACCENT);
            ui.add(if progress.fraction.is_none() { bar.animate(true) } else { bar });
        });
        let cancelling = self.cancel.load(Ordering::Relaxed);
        if self.mode == Mode::Install && buttons(ui, &[("Cancel", !cancelling)]).is_some() {
            self.cancel.store(true, Ordering::Relaxed);
        }
        false
    }

    fn done(&mut self, ui: &mut egui::Ui, progress: &Progress) -> bool {
        card(ui, |ui| match (&progress.error, self.mode) {
            (Some(error), _) => {
                ui.label(RichText::new("That didn't finish").strong().color(color::DANGER));
                ui.label(error);
            }
            (None, Mode::Install) => {
                ui.label(RichText::new(if self.updating { "Zakuro is updated" } else { "Zakuro is installed" }).strong());
                note(ui, "It's in the Start menu. Add your games from its Home screen.");
            }
            (None, Mode::Uninstall) => {
                ui.label(RichText::new("Zakuro is uninstalled").strong());
            }
        });
        for warning in &progress.warnings {
            ui.add_space(4.0);
            ui.label(RichText::new(warning).size(12.5).color(color::WARN));
        }
        let launch = self.mode == Mode::Install && progress.error.is_none();
        let failed = progress.error.is_some();
        let labels: &[(&str, bool)] = match (launch, failed) {
            (true, _) => &[("Start Zakuro", true), ("Close", true)],
            (false, true) => &[("Try again", true), ("Close", true)],
            (false, false) => &[("Close", true)],
        };
        match buttons(ui, labels) {
            Some(0) if launch => {
                let dir = PathBuf::from(self.dir.trim());
                let _ = std::process::Command::new(dir.join("zakuro.exe")).current_dir(&dir).spawn();
                true
            }
            Some(0) if failed => {
                self.page = Page::Choose;
                false
            }
            Some(_) => true,
            None => false,
        }
    }
}
