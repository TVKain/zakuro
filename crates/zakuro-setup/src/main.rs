//! Zakuro's setup: installs Zakuro for the current user, downloading the
//! Zig compiler that recompiling games uses, and, run as uninstall.exe or
//! with --uninstall, takes it away again, the player choosing whether their
//! games, saves and settings go too.

#![cfg_attr(windows, windows_subsystem = "windows")]

mod pad;
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
    pad: pad::Pad,
    /// the last input was a controller's or the keyboard's, not the pointer's.
    directional: bool,
    /// what a controller can reach, in the order it goes through them: the
    /// last frame's, top to bottom and left to right.
    order: Vec<egui::Id>,
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
            pad: pad::Pad::new(),
            directional: false,
            order: Vec::new(),
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
        let presses = self.pad.presses();
        if self.pad.connected() {
            // eframe sleeps without window events, which a controller isn't
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
        if self.draw(ui, &presses) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl Setup {
    /// the window's frame, after the controller's presses. whether it should close.
    fn draw(&mut self, ui: &mut egui::Ui, presses: &[pad::Press]) -> bool {
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
        self.order = ui.ctx().data_mut(|data| data.remove_temp::<Vec<egui::Id>>(egui::Id::new(ORDER))).unwrap_or_default();
        let mut close = self.controller(ui.ctx(), presses);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(color::BASE).inner_margin(Margin::same(28))).show(ui, |ui| {
            header(ui, self.mode);
            ui.add_space(16.0);
            close |= match (self.mode, self.page) {
                (Mode::Install, Page::Choose) => self.install_choices(ui),
                (Mode::Uninstall, Page::Choose) => self.uninstall_choices(ui),
                (_, Page::Working) => self.working(ui, &progress),
                (_, Page::Done) => self.done(ui, &progress),
            };
        });
        // the ring shows where a controller or the keyboard is, the pointer's
        // hover where the mouse is
        ui.ctx().input(|input| {
            if !presses.is_empty() || input.events.iter().any(|event| matches!(event, egui::Event::Key { .. })) {
                self.directional = true;
            } else if input.pointer.delta() != Vec2::ZERO || input.pointer.any_pressed() {
                self.directional = false;
            }
        });
        if self.directional {
            focus_ring(ui.ctx());
        }
        close
    }
}

/// an accent ring round what has the focus, the folder field aside, which
/// shows its own.
fn focus_ring(ctx: &egui::Context) {
    let Some(id) = ctx.memory(|memory| memory.focused()).filter(|id| *id != egui::Id::new(DIR_FIELD)) else { return };
    let Some(response) = ctx.read_response(id) else { return };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("focus-ring")));
    painter.rect_stroke(response.rect.expand(4.0), CornerRadius::same(10), Stroke::new(2.0, color::ACCENT), egui::StrokeKind::Outside);
}

/// the folder field, which A opens the folder picker on.
const DIR_FIELD: &str = "install-dir";
/// set when the main button should take the focus, for a controller that
/// finds nothing focused.
const FOCUS_MAIN: &str = "focus-main";

impl Setup {
    /// what the controller did this frame, as the keyboard would: a direction
    /// moves the focus, A presses what is focused, B cancels or closes.
    /// whether the window should close.
    fn controller(&mut self, ctx: &egui::Context, presses: &[pad::Press]) -> bool {
        // a controller only works the window in front
        if presses.is_empty() || !ctx.input(|input| input.viewport().focused.unwrap_or(true)) {
            return false;
        }
        let mut close = false;
        // focus asked for shows next frame, and presses can come several to one
        let mut focused = ctx.memory(|memory| memory.focused());
        for &press in presses {
            // up and left go back through the controls, down and right on
            let direction = match press {
                pad::Press::Up | pad::Press::Left => Some(-1),
                pad::Press::Down | pad::Press::Right => Some(1),
                _ => None,
            };
            match (press, direction) {
                (_, Some(_)) | (pad::Press::Accept, _) if focused.is_none() => {
                    // the first press finds the main button
                    ctx.data_mut(|data| data.insert_temp(egui::Id::new(FOCUS_MAIN), true));
                }
                (_, Some(step)) => {
                    let at = self.order.iter().position(|id| Some(*id) == focused);
                    let next = at.and_then(|at| at.checked_add_signed(step)).and_then(|next| self.order.get(next));
                    match (at, next) {
                        (_, Some(&id)) => {
                            ctx.memory_mut(|memory| memory.request_focus(id));
                            focused = Some(id);
                        }
                        // off the end it stays; lost, it finds the main button
                        (Some(_), None) => {}
                        (None, _) => ctx.data_mut(|data| { data.insert_temp(egui::Id::new(FOCUS_MAIN), true); }),
                    }
                }
                (pad::Press::Accept, _) if focused == Some(egui::Id::new(DIR_FIELD)) => self.pick_folder(),
                (pad::Press::Accept, _) => {
                    // a focused button or checkbox takes Enter as a click
                    ctx.input_mut(|input| {
                        input.events.push(egui::Event::Key {
                            key: egui::Key::Enter,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        })
                    });
                }
                _ => match self.page {
                    Page::Choose | Page::Done => close = true,
                    Page::Working => self.cancel.store(true, Ordering::Relaxed),
                },
            }
        }
        close
    }

    fn pick_folder(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().set_title("Where to install Zakuro").set_directory(&self.dir).pick_folder() {
            // a folder picked for it holds Zakuro in its own folder
            let named = folder.file_name().is_some_and(|name| name.eq_ignore_ascii_case("zakuro"));
            self.dir = if named { folder } else { folder.join("Zakuro") }.display().to_string();
        }
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

/// the order a controller goes through the controls in, as this frame draws
/// them.
const ORDER: &str = "controller-order";

/// puts a control drawn in the controller's order, unless it is disabled.
fn reachable(ui: &egui::Ui, response: &egui::Response) {
    if response.enabled() {
        ui.ctx().data_mut(|data| data.get_temp_mut_or_default::<Vec<egui::Id>>(egui::Id::new(ORDER)).push(response.id));
    }
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
    let mut drawn = Vec::new();
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        for (index, (label, enabled)) in labels.iter().enumerate().rev() {
            let text = RichText::new(*label).size(15.0);
            let button = if index == 0 { egui::Button::new(text.color(color::BASE).strong()).fill(color::ACCENT) } else { egui::Button::new(text) };
            let response = ui.add_enabled(*enabled, button.corner_radius(CornerRadius::same(8)).min_size(Vec2::new(110.0, 36.0)));
            if index == 0 && ui.ctx().data_mut(|data| data.remove_temp::<bool>(egui::Id::new(FOCUS_MAIN))).is_some() {
                response.request_focus();
            }
            if response.clicked() {
                clicked = Some(index);
            }
            drawn.push(response);
        }
    });
    // drawn from the right, they are gone through from the left
    for response in drawn.iter().rev() {
        reachable(ui, response);
    }
    clicked
}

impl Setup {
    fn install_choices(&mut self, ui: &mut egui::Ui) -> bool {
        card(ui, |ui| {
            ui.label(RichText::new("Install to").strong());
            ui.horizontal(|ui| {
                let field = egui::TextEdit::singleline(&mut self.dir)
                    .id(egui::Id::new(DIR_FIELD))
                    .desired_width(ui.available_width() - 100.0)
                    .margin(Margin::symmetric(8, 7));
                let field = ui.add(field);
                reachable(ui, &field);
                let change = ui.button("Change…");
                reachable(ui, &change);
                if change.clicked() {
                    self.pick_folder();
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
                let zig = ui.checkbox(&mut self.zig, RichText::new("Set up recompiling").strong());
                reachable(ui, &zig);
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
            let ticked = ui.checkbox(&mut self.desktop, "Add a desktop shortcut");
            reachable(ui, &ticked);
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
            let ticked = ui.checkbox(&mut self.games, format!("My games and their recompiled code ({})", size(|s| s.games)));
            reachable(ui, &ticked);
            if let Some(library) = sizes.as_ref().and_then(|sizes| sizes.library.as_ref()) {
                note(ui, format!("    In {}", library.display()));
            }
            let ticked = ui.checkbox(&mut self.saves, format!("My saves and save states ({})", size(|s| s.saves)));
            reachable(ui, &ticked);
            let ticked = ui.checkbox(&mut self.settings, format!("My settings, cheats, mods and texture packs ({})", size(|s| s.settings)));
            reachable(ui, &ticked);
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

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Pos2, Rect};
    use pad::Press;

    /// one frame of the window, after presses. whether it closes.
    fn frame(ctx: &egui::Context, setup: &mut Setup, presses: &[Press]) -> bool {
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 540.0))), ..Default::default() };
        let mut close = false;
        ctx.run_ui(input, |ui| close = setup.draw(ui, presses)).drop_without_applying_deltas();
        close
    }

    #[test]
    fn a_controller_works_the_uninstall_page() {
        let ctx = egui::Context::default();
        let mut setup = Setup::new(Mode::Uninstall, None);
        frame(&ctx, &mut setup, &[]);
        assert_eq!(ctx.memory(|memory| memory.focused()), None);
        // the first press finds the main button
        frame(&ctx, &mut setup, &[Press::Down]);
        frame(&ctx, &mut setup, &[]);
        let main = ctx.memory(|memory| memory.focused()).expect("the main button has the focus");
        // up goes to the choices above, and A ticks the one there
        frame(&ctx, &mut setup, &[Press::Up]);
        frame(&ctx, &mut setup, &[]);
        assert_ne!(ctx.memory(|memory| memory.focused()), Some(main));
        frame(&ctx, &mut setup, &[Press::Accept]);
        // the choice just above the buttons
        assert!(setup.settings && !setup.saves && !setup.games);
        assert_eq!(setup.page, Page::Choose);
        // B closes it, uninstalling nothing
        assert!(frame(&ctx, &mut setup, &[Press::Back]));
        assert_eq!(setup.page, Page::Choose);
    }

    #[test]
    fn presses_in_one_frame_each_move() {
        let ctx = egui::Context::default();
        let mut setup = Setup::new(Mode::Uninstall, None);
        frame(&ctx, &mut setup, &[]);
        frame(&ctx, &mut setup, &[Press::Down]);
        frame(&ctx, &mut setup, &[]);
        // from Uninstall, two up is the saves
        frame(&ctx, &mut setup, &[Press::Up, Press::Up]);
        frame(&ctx, &mut setup, &[Press::Accept]);
        assert!(setup.saves && !setup.settings && !setup.games);
    }

    #[test]
    fn b_while_installing_cancels_it() {
        let ctx = egui::Context::default();
        let mut setup = Setup::new(Mode::Install, None);
        setup.page = Page::Working;
        assert!(!frame(&ctx, &mut setup, &[Press::Back]));
        assert!(setup.cancel.load(Ordering::Relaxed));
    }
}
