//! the library, the menu over a game and the settings, drawn with egui.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use egui::{Align, Color32, Layout, RichText, Vec2};
use winit::keyboard::KeyCode;
use zakuro_core::services::keyboard::Request;

use crate::library::{Game, Library, ICON_SIZE};
use crate::recompile::{Job, Stage, Wanted};
use crate::gamepad::button_name;
use crate::settings::{
    game_key, CameraSource, Filter, GameSettings, Keys, Language, MicrophoneSource, Orientation, PadButtons, Renderer, Screens, Settings, Stereo3d, TopShape,
    WindowMode,
};

/// what the user asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Play(PathBuf),
    /// recompile these games of the library, one after the other.
    Recompile(Vec<Wanted>),
    /// download a compiler, then recompile these games of the library with
    /// it.
    DownloadCompiler(Vec<Wanted>),
    /// show the mods folder of the game with this program id.
    Mods(u64),
    /// stop recompiling the game with this id, or waiting to.
    CancelRecompile(u64),
    /// leave only the game recompiling now, none of those waiting.
    CancelWaiting,
    ChooseFolder,
    ChooseBackground,
    /// pick the picture the cameras see.
    ChooseCameraPicture,
    /// save the game's state to the slot chosen, or load it from there.
    SaveState,
    LoadState,
    /// open a room and join it, join another's, or leave the one joined.
    HostRoom,
    JoinRoom,
    LeaveRoom,
    /// take part in the Download Play offer at this place in the list, or
    /// leave the one taken part in.
    DlpJoin(usize),
    DlpLeave,
    Rescan,
    Resume,
    Reset,
    Library,
    Fullscreen,
    /// copy what a report on the game wants.
    CopyReport,
    Quit,
    /// the settings changed, to save and apply.
    Settings,
    /// close the game's keyboard with this text and the button pressed.
    Keyboard(String, usize),
    /// turn the running game's cheat at this index on or off.
    Cheat(usize, bool),
    /// add a cheat, its name and its code, to the running game.
    AddCheat(String, String),
    /// read the running game's cheats from their file again.
    ReloadCheats,
    /// show the cheats folder.
    CheatsFolder,
}

/// the active tab in the settings dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsTab {
    #[default]
    Graphics,
    Controller,
    Audio,
    System,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 4] = [SettingsTab::Graphics, SettingsTab::Controller, SettingsTab::Audio, SettingsTab::System];

    pub fn name(self) -> &'static str {
        match self {
            SettingsTab::Graphics => "🎮 Graphics",
            SettingsTab::Controller => "🕹 Controls",
            SettingsTab::Audio => "🔊 Audio",
            SettingsTab::System => "⚙ System",
        }
    }

    pub fn next(self) -> Self {
        match self {
            SettingsTab::Graphics => SettingsTab::Controller,
            SettingsTab::Controller => SettingsTab::Audio,
            SettingsTab::Audio => SettingsTab::System,
            SettingsTab::System => SettingsTab::Graphics,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            SettingsTab::Graphics => SettingsTab::System,
            SettingsTab::Controller => SettingsTab::Graphics,
            SettingsTab::Audio => SettingsTab::Controller,
            SettingsTab::System => SettingsTab::Audio,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameSettingsTab {
    #[default]
    Graphics,
    System,
}

impl GameSettingsTab {
    pub const ALL: [GameSettingsTab; 2] = [GameSettingsTab::Graphics, GameSettingsTab::System];

    pub fn name(self) -> &'static str {
        match self {
            GameSettingsTab::Graphics => "🎮 Graphics",
            GameSettingsTab::System => "⚙ System",
        }
    }

    pub fn next(self) -> Self {
        match self {
            GameSettingsTab::Graphics => GameSettingsTab::System,
            GameSettingsTab::System => GameSettingsTab::Graphics,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            GameSettingsTab::Graphics => GameSettingsTab::System,
            GameSettingsTab::System => GameSettingsTab::Graphics,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectTarget {
    Renderer,
    Layout,
    Orientation,
    Filter,
    TopShape,
    Stereo,
    Microphone,
    WindowMode,
    Language,
    Camera,
}

#[derive(Clone, Debug)]
pub struct SelectPopup {
    pub target: SelectTarget,
    pub for_game: Option<u64>,
    pub title: &'static str,
    pub options: Vec<String>,
    pub selected: usize,
}

impl SelectPopup {
    pub fn global(target: SelectTarget, title: &'static str, options: Vec<String>, selected: usize) -> Self {
        Self {
            target,
            for_game: None,
            title,
            options,
            selected,
        }
    }

    pub fn game(target: SelectTarget, program_id: u64, title: &'static str, options: Vec<String>, selected: usize) -> Self {
        Self {
            target,
            for_game: Some(program_id),
            title,
            options,
            selected,
        }
    }
}

#[derive(Default)]
pub struct Menus {
    /// the menu over a running game.
    pub menu_open: bool,
    /// the cheats window, and the cheat being written in it.
    pub cheats_open: bool,
    cheat_name: String,
    cheat_code: String,
    pub settings_open: bool,
    /// the game whose own settings are open, its id and its name.
    pub game_settings: Option<(u64, String)>,
    /// the binding waiting for a key, by its place in Keys::all_mut.
    pub rebinding: Option<usize>,
    /// the controller binding waiting for a button, by its place in
    /// PadButtons::all_mut.
    pub rebinding_pad: Option<usize>,
    /// something to tell the user, until they close it.
    pub message: Option<String>,
    /// something to tell the user for a moment, over the game, and since
    /// when.
    pub toast: Option<(String, std::time::Instant)>,
    /// the slot states are saved to and loaded from, 1 to 9.
    pub slot: u8,
    /// the multiplayer window, and the password typed in it.
    pub multiplayer_open: bool,
    pub room_password: String,
    /// the icons of the games offered for Download Play.
    dlp_icons: HashMap<u64, egui::TextureHandle>,
    /// a report on the game, to copy from the message it goes with, and
    /// from no other.
    pub report: Option<(String, String)>,
    /// what is being typed into the game's keyboard, and what it asked for.
    typing: Option<(Request, String)>,
    /// the library's games to recompile once the player agrees to download
    /// a compiler for them.
    pub compiler_offer: Option<Vec<Wanted>>,
    icons: HashMap<PathBuf, egui::TextureHandle>,
    background: Background,
    /// the game currently selected in the library by controller or keyboard.
    pub selected_game: usize,
    /// the action currently selected in the pause menu by controller.
    pub pause_selected: usize,
    /// whether the library scroll area should scroll to reveal selected_game.
    pub scroll_to_selected: bool,
    /// the tab currently selected in the settings dialog.
    pub settings_tab: SettingsTab,
    /// the setting row currently selected by controller in settings.
    pub settings_selected: usize,
    /// whether the settings scroll area should scroll to reveal settings_selected.
    pub settings_scroll_to_selected: bool,
    /// active selection popup dialog when choosing options via controller/UI.
    pub select_popup: Option<SelectPopup>,
    /// the tab currently selected in the game settings dialog.
    pub game_settings_tab: GameSettingsTab,
    /// the setting row currently selected by controller in game settings.
    pub game_settings_selected: usize,
    /// whether the game settings scroll area should scroll to reveal game_settings_selected.
    pub game_settings_scroll_to_selected: bool,
    /// whether a modal dialog was just opened and needs initial widget focus.
    pub modal_just_opened: bool,
}

/// the room the player is in, as the multiplayer window shows it.
pub struct RoomView {
    pub name: String,
    /// each member's nickname and the game they play.
    pub members: Vec<(String, String)>,
    /// the port of the room this program holds open, when it is its own.
    pub port: Option<u16>,
    /// Download Play, while no game runs.
    pub download_play: Option<DlpView>,
}

/// the Download Play offers in the room, and where the guest is.
pub struct DlpView {
    pub offers: Vec<OfferView>,
    /// what the guest is doing, and how far along, while it takes part.
    pub doing: Option<(String, Option<f32>)>,
}

pub struct OfferView {
    pub child: u64,
    pub title: String,
    pub host: String,
    /// RGBA, ICON_SIZE square.
    pub icon: Option<Vec<u8>>,
    pub guests: u8,
    pub most: u8,
    /// why the player can't take part, when they can't.
    pub blocked: Option<String>,
}

/// the picture behind the library, decoded away from the interface.
#[derive(Default)]
struct Background {
    /// the file shown or being read.
    path: Option<PathBuf>,
    texture: Option<egui::TextureHandle>,
    loading: Option<Receiver<Result<egui::ColorImage, String>>>,
}

/// the longest side a background is kept at, bigger ones are scaled down.
const BACKGROUND_SIDE: u32 = 2560;

impl Background {
    /// the texture for path, starting to read it when it changed, none until
    /// it is ready or when there is no picture.
    fn get(&mut self, ctx: &egui::Context, path: Option<&Path>) -> Result<Option<&egui::TextureHandle>, String> {
        if self.path.as_deref() != path {
            self.path = path.map(Path::to_owned);
            self.texture = None;
            self.loading = path.map(|path| {
                let (send, receive) = channel();
                let path = path.to_owned();
                std::thread::spawn(move || {
                    let _ = send.send(decode(&path));
                });
                receive
            });
        }
        if let Some(receive) = &self.loading {
            match receive.try_recv() {
                Ok(image) => {
                    self.loading = None;
                    self.texture = Some(ctx.load_texture("library background", image?, egui::TextureOptions::LINEAR));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint(),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.loading = None,
            }
        }
        Ok(self.texture.as_ref())
    }
}

fn decode(path: &Path) -> Result<egui::ColorImage, String> {
    let image = image::open(path).map_err(|error| format!("could not open {}, {error}", path.display()))?;
    let image = if image.width().max(image.height()) > BACKGROUND_SIDE {
        image.resize(BACKGROUND_SIDE, BACKGROUND_SIDE, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let rgba = image.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

/// the part of a picture that fills an area without stretching it, cutting
/// off what sticks out on either side.
fn cover(area: Vec2, picture: [usize; 2]) -> egui::Rect {
    let (width, height) = (picture[0].max(1) as f32, picture[1].max(1) as f32);
    let area_ratio = area.x / area.y.max(1.0);
    let picture_ratio = width / height;
    if area_ratio > picture_ratio {
        // wider area, the picture's top and bottom are cut
        let shown = picture_ratio / area_ratio;
        egui::Rect::from_min_max(egui::pos2(0.0, (1.0 - shown) / 2.0), egui::pos2(1.0, (1.0 + shown) / 2.0))
    } else {
        let shown = area_ratio / picture_ratio;
        egui::Rect::from_min_max(egui::pos2((1.0 - shown) / 2.0, 0.0), egui::pos2((1.0 + shown) / 2.0, 1.0))
    }
}

/// a small pill-shaped tag badge.
fn pill_badge(ui: &mut egui::Ui, text: &str, bg: Color32, fg: Color32) {
    egui::Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).strong().color(fg));
        });
}

/// a handheld controller button prompt badge.
fn button_hint(ui: &mut egui::Ui, button: &str, label: &str, btn_color: Color32) {
    ui.horizontal(|ui| {
        egui::Frame::new()
            .fill(btn_color)
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(6, 2))
            .show(ui, |ui| {
                ui.label(RichText::new(button).size(11.0).strong().color(Color32::WHITE));
            });
        ui.label(RichText::new(label).size(12.0).color(Color32::from_rgb(215, 222, 235)));
    });
}

fn nav_button(ui: &mut egui::Ui, focused: bool, text: &str) -> (bool, bool) {
    let btn = egui::Button::new(RichText::new(text).size(14.0));
    let resp = if focused {
        ui.add_sized([260.0, 32.0], btn.fill(Color32::from_rgb(37, 99, 235)).stroke(egui::Stroke::new(1.5, Color32::from_rgb(147, 197, 253))))
    } else {
        ui.add_sized([260.0, 32.0], btn)
    };
    (resp.clicked(), resp.hovered())
}

fn setting_row(ui: &mut egui::Ui, selected: bool, should_scroll: bool, content: impl FnOnce(&mut egui::Ui)) -> bool {
    let mut frame = egui::Frame::new()
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 5));
    if selected {
        frame = frame
            .fill(Color32::from_rgb(30, 48, 75))
            .stroke(egui::Stroke::new(1.5, Color32::from_rgb(56, 189, 248)));
    } else {
        frame = frame.fill(Color32::from_rgb(20, 23, 30));
    }
    let res = frame.show(ui, content);
    if selected && should_scroll {
        res.response.scroll_to_me(Some(egui::Align::Center));
    }
    ui.add_space(3.0);
    res.response.clicked()
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum GameRowAction {
    None,
    Reset,
    Edit,
}

fn game_setting_row(
    ui: &mut egui::Ui,
    selected: bool,
    should_scroll: bool,
    name: &str,
    is_custom: bool,
    custom_val: &str,
    global_val: &str,
) -> (GameRowAction, bool) {
    let mut action = GameRowAction::None;
    let clicked = setting_row(ui, selected, should_scroll, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(name).strong().size(13.0).color(Color32::from_rgb(225, 232, 245)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if is_custom {
                    let reset_btn = egui::Button::new(RichText::new("↺ Global").size(11.0).color(Color32::from_rgb(148, 163, 184)))
                        .fill(Color32::from_rgb(28, 33, 44))
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(51, 65, 85)));
                    if ui.add_sized([65.0, 26.0], reset_btn).on_hover_text("Revert to following Global Setting").clicked() {
                        action = GameRowAction::Reset;
                    }
                    let badge_text = format!("CUSTOM: {custom_val}");
                    let val_btn = egui::Button::new(RichText::new(badge_text).size(12.0).strong().color(Color32::from_rgb(56, 189, 248)))
                        .fill(Color32::from_rgb(20, 38, 62))
                        .stroke(egui::Stroke::new(1.2, Color32::from_rgb(56, 189, 248)));
                    if ui.add_sized([160.0, 26.0], val_btn).clicked() {
                        action = GameRowAction::Edit;
                    }
                } else {
                    let badge_text = format!("GLOBAL: {global_val}");
                    let val_btn = egui::Button::new(RichText::new(badge_text).size(12.0).color(Color32::from_rgb(148, 163, 184)))
                        .fill(Color32::from_rgb(24, 28, 38))
                        .stroke(egui::Stroke::new(1.0, Color32::from_rgb(51, 65, 85)));
                    if ui.add_sized([160.0, 26.0], val_btn).on_hover_text("Currently following Global Setting. Click to override for this game.").clicked() {
                        action = GameRowAction::Edit;
                    }
                }
            });
        });
    });
    if action == GameRowAction::None && clicked {
        action = GameRowAction::Edit;
    }
    (action, clicked)
}

fn cycle<T: Copy + PartialEq>(list: &[T], current: T, delta: i32) -> T {
    let len = list.len();
    let idx = list.iter().position(|&x| x == current).unwrap_or(0);
    let next = if delta > 0 { (idx + 1) % len } else { (idx + len - 1) % len };
    list[next]
}

impl Menus {
    /// whether any modal window is open (settings, cheats, multiplayer, game settings, message).
    pub fn modal_open(&self) -> bool {
        self.settings_open || self.cheats_open || self.multiplayer_open || self.game_settings.is_some() || self.message.is_some() || self.select_popup.is_some()
    }

    /// closes any open modal window.
    pub fn close_modals(&mut self) {
        if self.select_popup.is_some() {
            self.select_popup = None;
            return;
        }
        self.settings_open = false;
        self.cheats_open = false;
        self.multiplayer_open = false;
        self.game_settings = None;
        self.message = None;
        self.modal_just_opened = false;
    }

    /// opens the settings dialog and flags initial focus.
    pub fn open_settings(&mut self) {
        self.settings_open = true;
        self.settings_selected = 0;
        self.settings_scroll_to_selected = true;
        self.modal_just_opened = true;
    }

    /// opens the game-specific settings dialog and flags initial focus.
    pub fn open_game_settings(&mut self, program_id: u64, name: String) {
        self.game_settings = Some((program_id, name));
        self.game_settings_tab = GameSettingsTab::Graphics;
        self.game_settings_selected = 0;
        self.game_settings_scroll_to_selected = true;
        self.modal_just_opened = true;
    }

    pub fn game_settings_up(&mut self) {
        self.game_settings_selected = self.game_settings_selected.saturating_sub(1);
        self.game_settings_scroll_to_selected = true;
    }

    pub fn game_settings_down(&mut self) {
        let max = match self.game_settings_tab {
            GameSettingsTab::Graphics => 12,
            GameSettingsTab::System => 6,
        };
        self.game_settings_selected = (self.game_settings_selected + 1).min(max - 1);
        self.game_settings_scroll_to_selected = true;
    }

    pub fn game_settings_adjust(&mut self, settings: &mut Settings, program_id: u64, delta: i32) -> Option<Action> {
        let key = game_key(program_id);
        let entry = settings.per_game.entry(key).or_default();
        match self.game_settings_tab {
            GameSettingsTab::Graphics => match self.game_settings_selected {
                1 => {
                    let cur = entry.resolution.unwrap_or(settings.resolution);
                    let new_res = (cur as i32 + delta).clamp(1, 8) as u32;
                    entry.resolution = Some(new_res);
                    Some(Action::Settings)
                }
                10 => {
                    let cur = entry.depth_3d.unwrap_or(settings.depth_3d);
                    let new_depth = (cur as i32 + delta * 5).clamp(0, 100) as u32;
                    entry.depth_3d = Some(new_depth);
                    Some(Action::Settings)
                }
                _ => None,
            },
            GameSettingsTab::System => match self.game_settings_selected {
                0 => {
                    let cur = entry.show_fps.unwrap_or(settings.show_fps);
                    entry.show_fps = Some(!cur);
                    Some(Action::Settings)
                }
                2 => {
                    let cur = entry.window_mode.unwrap_or(settings.window_mode);
                    entry.window_mode = Some(cycle(&WindowMode::ALL, cur, delta));
                    Some(Action::Settings)
                }
                3 => {
                    let cur = entry.language.unwrap_or(settings.language);
                    entry.language = Some(cycle(&Language::ALL, cur, delta));
                    Some(Action::Settings)
                }
                4 => {
                    let cur = entry.recompiled.unwrap_or(settings.recompiled);
                    entry.recompiled = Some(!cur);
                    Some(Action::Settings)
                }
                _ => None,
            },
        }
    }

    pub fn game_settings_reset_selected(&mut self, settings: &mut Settings, program_id: u64) -> Option<Action> {
        let key = game_key(program_id);
        let mut remove_all = false;
        if let Some(entry) = settings.per_game.get_mut(&key) {
            match self.game_settings_tab {
                GameSettingsTab::Graphics => match self.game_settings_selected {
                    0 => entry.hardware_rasterizer = None,
                    1 => entry.resolution = None,
                    2 => entry.texture_packs = None,
                    3 => entry.frame_interpolation = None,
                    4 => entry.layout = None,
                    5 => entry.orientation = None,
                    6 => entry.filter = None,
                    7 => entry.integer_scale = None,
                    8 => entry.top_shape = None,
                    9 => entry.stereo = None,
                    10 => entry.depth_3d = None,
                    11 => remove_all = true,
                    _ => {}
                },
                GameSettingsTab::System => match self.game_settings_selected {
                    0 => entry.show_fps = None,
                    1 => {}
                    2 => entry.window_mode = None,
                    3 => entry.language = None,
                    4 => entry.recompiled = None,
                    5 => remove_all = true,
                    _ => {}
                },
            }
            if *entry == GameSettings::default() {
                remove_all = true;
            }
        }
        if remove_all {
            settings.per_game.remove(&key);
        }
        Some(Action::Settings)
    }

    pub fn game_settings_activate(&mut self, settings: &mut Settings, program_id: u64) -> Option<Action> {
        let key = game_key(program_id);
        match self.game_settings_tab {
            GameSettingsTab::Graphics => match self.game_settings_selected {
                0 => {
                    let entry = settings.per_game.entry(key).or_default();
                    let cur = entry.hardware_rasterizer.unwrap_or(settings.hardware_rasterizer);
                    entry.hardware_rasterizer = Some(!cur);
                    Some(Action::Settings)
                }
                1 => {
                    let entry = settings.per_game.entry(key).or_default();
                    let cur = entry.resolution.unwrap_or(settings.resolution);
                    entry.resolution = Some(if cur >= 8 { 1 } else { cur + 1 });
                    Some(Action::Settings)
                }
                2 => {
                    let entry = settings.per_game.entry(key).or_default();
                    let cur = entry.texture_packs.unwrap_or(settings.texture_packs);
                    entry.texture_packs = Some(!cur);
                    Some(Action::Settings)
                }
                3 => {
                    let entry = settings.per_game.entry(key).or_default();
                    let cur = entry.frame_interpolation.unwrap_or(settings.frame_interpolation);
                    entry.frame_interpolation = Some(!cur);
                    Some(Action::Settings)
                }
                4 => {
                    let cur = settings.per_game.get(&key).and_then(|g| g.layout).unwrap_or(settings.layout);
                    self.select_popup = Some(SelectPopup::game(
                        SelectTarget::Layout,
                        program_id,
                        "Screen Layout",
                        Screens::ALL.iter().map(|s| s.name().to_string()).collect(),
                        Screens::ALL.iter().position(|&s| s == cur).unwrap_or(0),
                    ));
                    None
                }
                5 => {
                    let cur = settings.per_game.get(&key).and_then(|g| g.orientation).unwrap_or(settings.orientation);
                    self.select_popup = Some(SelectPopup::game(
                        SelectTarget::Orientation,
                        program_id,
                        "Screen Orientation",
                        Orientation::ALL.iter().map(|o| o.name().to_string()).collect(),
                        Orientation::ALL.iter().position(|&o| o == cur).unwrap_or(0),
                    ));
                    None
                }
                6 => {
                    let cur = settings.per_game.get(&key).and_then(|g| g.filter).unwrap_or(settings.filter);
                    self.select_popup = Some(SelectPopup::game(
                        SelectTarget::Filter,
                        program_id,
                        "Texture Filter",
                        Filter::ALL.iter().map(|f| f.name().to_string()).collect(),
                        Filter::ALL.iter().position(|&f| f == cur).unwrap_or(0),
                    ));
                    None
                }
                7 => {
                    let entry = settings.per_game.entry(key).or_default();
                    let cur = entry.integer_scale.unwrap_or(settings.integer_scale);
                    entry.integer_scale = Some(!cur);
                    Some(Action::Settings)
                }
                8 => {
                    let cur = settings.per_game.get(&key).and_then(|g| g.top_shape).unwrap_or(settings.top_shape);
                    self.select_popup = Some(SelectPopup::game(
                        SelectTarget::TopShape,
                        program_id,
                        "Top Screen Aspect Ratio",
                        TopShape::ALL.iter().map(|s| s.name().to_string()).collect(),
                        TopShape::ALL.iter().position(|&s| s == cur).unwrap_or(0),
                    ));
                    None
                }
                9 => {
                    let cur = settings.per_game.get(&key).and_then(|g| g.stereo).unwrap_or(settings.stereo);
                    self.select_popup = Some(SelectPopup::game(
                        SelectTarget::Stereo,
                        program_id,
                        "Stereo 3D Mode",
                        Stereo3d::ALL.iter().map(|s| s.name().to_string()).collect(),
                        Stereo3d::ALL.iter().position(|&s| s == cur).unwrap_or(0),
                    ));
                    None
                }
                10 => {
                    let entry = settings.per_game.entry(key).or_default();
                    let cur = entry.depth_3d.unwrap_or(settings.depth_3d);
                    entry.depth_3d = Some(if cur >= 100 { 0 } else { cur + 10 });
                    Some(Action::Settings)
                }
                11 => {
                    settings.per_game.remove(&key);
                    Some(Action::Settings)
                }
                _ => None,
            },
            GameSettingsTab::System => match self.game_settings_selected {
                0 => {
                    let entry = settings.per_game.entry(key).or_default();
                    let cur = entry.show_fps.unwrap_or(settings.show_fps);
                    entry.show_fps = Some(!cur);
                    Some(Action::Settings)
                }
                1 => Some(Action::Fullscreen),
                2 => {
                    let cur = settings.per_game.get(&key).and_then(|g| g.window_mode).unwrap_or(settings.window_mode);
                    self.select_popup = Some(SelectPopup::game(
                        SelectTarget::WindowMode,
                        program_id,
                        "Window Mode",
                        WindowMode::ALL.iter().map(|w| w.name().to_string()).collect(),
                        WindowMode::ALL.iter().position(|&w| w == cur).unwrap_or(0),
                    ));
                    None
                }
                3 => {
                    let cur = settings.per_game.get(&key).and_then(|g| g.language).unwrap_or(settings.language);
                    self.select_popup = Some(SelectPopup::game(
                        SelectTarget::Language,
                        program_id,
                        "Console Language",
                        Language::ALL.iter().map(|l| l.name().to_string()).collect(),
                        Language::ALL.iter().position(|&l| l == cur).unwrap_or(0),
                    ));
                    None
                }
                4 => {
                    let entry = settings.per_game.entry(key).or_default();
                    let cur = entry.recompiled.unwrap_or(settings.recompiled);
                    entry.recompiled = Some(!cur);
                    Some(Action::Settings)
                }
                5 => {
                    settings.per_game.remove(&key);
                    Some(Action::Settings)
                }
                _ => None,
            },
        }
    }

    pub fn settings_max_rows(&self) -> usize {
        match self.settings_tab {
            SettingsTab::Graphics => 13,
            SettingsTab::Controller => 17,
            SettingsTab::Audio => 3,
            SettingsTab::System => 10,
        }
    }

    pub fn settings_up(&mut self) {
        self.settings_selected = self.settings_selected.saturating_sub(1);
        self.settings_scroll_to_selected = true;
    }

    pub fn settings_down(&mut self) {
        let max = self.settings_max_rows();
        if max > 0 {
            self.settings_selected = (self.settings_selected + 1).min(max - 1);
            self.settings_scroll_to_selected = true;
        }
    }

    pub fn settings_adjust(&mut self, settings: &mut Settings, delta: i32) -> Option<Action> {
        match self.settings_tab {
            SettingsTab::Graphics => match self.settings_selected {
                0 => settings.renderer = cycle(&Renderer::ALL, settings.renderer, delta),
                1 => settings.hardware_rasterizer = !settings.hardware_rasterizer,
                2 => settings.resolution = (settings.resolution as i32 + delta).clamp(1, 8) as u32,
                3 => settings.texture_packs = !settings.texture_packs,
                4 => settings.frame_interpolation = !settings.frame_interpolation,
                5 => settings.layout = cycle(&Screens::ALL, settings.layout, delta),
                6 => settings.orientation = cycle(&Orientation::ALL, settings.orientation, delta),
                7 => settings.filter = cycle(&Filter::ALL, settings.filter, delta),
                8 => settings.integer_scale = !settings.integer_scale,
                9 => settings.top_shape = cycle(&TopShape::ALL, settings.top_shape, delta),
                10 => settings.stereo = cycle(&Stereo3d::ALL, settings.stereo, delta),
                11 => settings.depth_3d = (settings.depth_3d as i32 + delta * 5).clamp(0, 100) as u32,
                12 => settings.swap_eyes = !settings.swap_eyes,
                _ => {}
            },
            SettingsTab::Controller => {}
            SettingsTab::Audio => match self.settings_selected {
                0 => settings.volume = (settings.volume + (delta as f32 * 0.05)).clamp(0.0, 1.0),
                1 => settings.mute = !settings.mute,
                2 => settings.microphone = cycle(&MicrophoneSource::ALL, settings.microphone, delta),
                _ => {}
            },
            SettingsTab::System => match self.settings_selected {
                0 => settings.window_mode = cycle(&WindowMode::ALL, settings.window_mode, delta),
                1 => settings.scale = (settings.scale as i32 + delta).clamp(1, 6) as u32,
                2 => settings.show_fps = !settings.show_fps,
                3 => {}
                4 => settings.language = cycle(&Language::ALL, settings.language, delta),
                5 => settings.recompiled = !settings.recompiled,
                6 => settings.camera = cycle(&CameraSource::ALL, settings.camera, delta),
                7 => {}
                8 => {}
                9 => settings.background_opacity = (settings.background_opacity + (delta as f32 * 0.05)).clamp(0.0, 1.0),
                _ => {}
            },
        }
        None
    }

    pub fn settings_activate(&mut self, settings: &mut Settings) -> Option<Action> {
        match self.settings_tab {
            SettingsTab::Graphics => match self.settings_selected {
                0 => {
                    self.select_popup = Some(SelectPopup::global(
                        SelectTarget::Renderer,
                        "Presenter / Backend",
                        Renderer::ALL.iter().map(|r| r.name().to_string()).collect(),
                        Renderer::ALL.iter().position(|&r| r == settings.renderer).unwrap_or(0),
                    ));
                    None
                }
                1 => {
                    settings.hardware_rasterizer = !settings.hardware_rasterizer;
                    None
                }
                2 => None,
                3 => {
                    settings.texture_packs = !settings.texture_packs;
                    None
                }
                4 => {
                    settings.frame_interpolation = !settings.frame_interpolation;
                    None
                }
                5 => {
                    self.select_popup = Some(SelectPopup::global(
                        SelectTarget::Layout,
                        "Screen Layout",
                        Screens::ALL.iter().map(|s| s.name().to_string()).collect(),
                        Screens::ALL.iter().position(|&s| s == settings.layout).unwrap_or(0),
                    ));
                    None
                }
                6 => {
                    self.select_popup = Some(SelectPopup::global(
                        SelectTarget::Orientation,
                        "Screen Orientation",
                        Orientation::ALL.iter().map(|o| o.name().to_string()).collect(),
                        Orientation::ALL.iter().position(|&o| o == settings.orientation).unwrap_or(0),
                    ));
                    None
                }
                7 => {
                    self.select_popup = Some(SelectPopup::global(
                        SelectTarget::Filter,
                        "Filter",
                        Filter::ALL.iter().map(|f| f.name().to_string()).collect(),
                        Filter::ALL.iter().position(|&f| f == settings.filter).unwrap_or(0),
                    ));
                    None
                }
                8 => {
                    settings.integer_scale = !settings.integer_scale;
                    None
                }
                9 => {
                    self.select_popup = Some(SelectPopup::global(
                        SelectTarget::TopShape,
                        "Top Screen Aspect",
                        TopShape::ALL.iter().map(|s| s.name().to_string()).collect(),
                        TopShape::ALL.iter().position(|&s| s == settings.top_shape).unwrap_or(0),
                    ));
                    None
                }
                10 => {
                    self.select_popup = Some(SelectPopup::global(
                        SelectTarget::Stereo,
                        "3D Stereoscopy",
                        Stereo3d::ALL.iter().map(|s| s.name().to_string()).collect(),
                        Stereo3d::ALL.iter().position(|&s| s == settings.stereo).unwrap_or(0),
                    ));
                    None
                }
                11 => None,
                12 => {
                    settings.swap_eyes = !settings.swap_eyes;
                    None
                }
                _ => None,
            },
            SettingsTab::Controller => match self.settings_selected {
                i if i < 15 => {
                    self.rebinding_pad = Some(i);
                    self.rebinding = None;
                    None
                }
                15 => {
                    settings.pad = PadButtons::default();
                    self.rebinding_pad = None;
                    None
                }
                16 => {
                    settings.keys = Keys::default();
                    self.rebinding = None;
                    None
                }
                _ => None,
            },
            SettingsTab::Audio => match self.settings_selected {
                0 => None,
                1 => {
                    settings.mute = !settings.mute;
                    None
                }
                2 => {
                    self.select_popup = Some(SelectPopup::global(
                        SelectTarget::Microphone,
                        "Microphone Source",
                        MicrophoneSource::ALL.iter().map(|m| m.name().to_string()).collect(),
                        MicrophoneSource::ALL.iter().position(|&m| m == settings.microphone).unwrap_or(0),
                    ));
                    None
                }
                _ => None,
            },
            SettingsTab::System => match self.settings_selected {
                0 => {
                    self.select_popup = Some(SelectPopup::global(
                        SelectTarget::WindowMode,
                        "Window Mode",
                        WindowMode::ALL.iter().map(|w| w.name().to_string()).collect(),
                        WindowMode::ALL.iter().position(|&w| w == settings.window_mode).unwrap_or(0),
                    ));
                    None
                }
                1 => None,
                2 => {
                    settings.show_fps = !settings.show_fps;
                    None
                }
                3 => Some(Action::Fullscreen),
                4 => {
                    self.select_popup = Some(SelectPopup::global(
                        SelectTarget::Language,
                        "System Language",
                        Language::ALL.iter().map(|l| l.name().to_string()).collect(),
                        Language::ALL.iter().position(|&l| l == settings.language).unwrap_or(0),
                    ));
                    None
                }
                5 => {
                    settings.recompiled = !settings.recompiled;
                    None
                }
                6 => {
                    self.select_popup = Some(SelectPopup::global(
                        SelectTarget::Camera,
                        "Camera Source",
                        CameraSource::ALL.iter().map(|c| c.name().to_string()).collect(),
                        CameraSource::ALL.iter().position(|&c| c == settings.camera).unwrap_or(0),
                    ));
                    None
                }
                7 => Some(Action::ChooseCameraPicture),
                8 => Some(Action::CopyReport),
                9 => None,
                _ => None,
            },
        }
    }

    pub fn confirm_select_popup(&mut self, settings: &mut Settings) -> Option<Action> {
        let popup = self.select_popup.take()?;
        let idx = popup.selected;
        if let Some(program_id) = popup.for_game {
            let key = game_key(program_id);
            let entry = settings.per_game.entry(key).or_default();
            match popup.target {
                SelectTarget::Layout => {
                    if let Some(&l) = Screens::ALL.get(idx) {
                        entry.layout = Some(l);
                    }
                }
                SelectTarget::Orientation => {
                    if let Some(&o) = Orientation::ALL.get(idx) {
                        entry.orientation = Some(o);
                    }
                }
                SelectTarget::Filter => {
                    if let Some(&f) = Filter::ALL.get(idx) {
                        entry.filter = Some(f);
                    }
                }
                SelectTarget::TopShape => {
                    if let Some(&s) = TopShape::ALL.get(idx) {
                        entry.top_shape = Some(s);
                    }
                }
                SelectTarget::Stereo => {
                    if let Some(&s) = Stereo3d::ALL.get(idx) {
                        entry.stereo = Some(s);
                    }
                }
                SelectTarget::Language => {
                    if let Some(&l) = Language::ALL.get(idx) {
                        entry.language = Some(l);
                    }
                }
                SelectTarget::WindowMode => {
                    if let Some(&w) = WindowMode::ALL.get(idx) {
                        entry.window_mode = Some(w);
                    }
                }
                _ => {}
            }
        } else {
            match popup.target {
                SelectTarget::Renderer => {
                    if let Some(&r) = Renderer::ALL.get(idx) {
                        settings.renderer = r;
                    }
                }
                SelectTarget::Layout => {
                    if let Some(&l) = Screens::ALL.get(idx) {
                        settings.layout = l;
                    }
                }
                SelectTarget::Orientation => {
                    if let Some(&o) = Orientation::ALL.get(idx) {
                        settings.orientation = o;
                    }
                }
                SelectTarget::Filter => {
                    if let Some(&f) = Filter::ALL.get(idx) {
                        settings.filter = f;
                    }
                }
                SelectTarget::TopShape => {
                    if let Some(&s) = TopShape::ALL.get(idx) {
                        settings.top_shape = s;
                    }
                }
                SelectTarget::Stereo => {
                    if let Some(&s) = Stereo3d::ALL.get(idx) {
                        settings.stereo = s;
                    }
                }
                SelectTarget::Microphone => {
                    if let Some(&m) = MicrophoneSource::ALL.get(idx) {
                        settings.microphone = m;
                    }
                }
                SelectTarget::WindowMode => {
                    if let Some(&w) = WindowMode::ALL.get(idx) {
                        settings.window_mode = w;
                    }
                }
                SelectTarget::Language => {
                    if let Some(&l) = Language::ALL.get(idx) {
                        settings.language = l;
                    }
                }
                SelectTarget::Camera => {
                    if let Some(&c) = CameraSource::ALL.get(idx) {
                        settings.camera = c;
                    }
                }
            }
        }
        Some(Action::Settings)
    }

    pub fn show_select_popup(&mut self, ctx: &egui::Context, settings: &mut Settings) -> Vec<Action> {
        let mut actions = Vec::new();
        let Some(popup) = self.select_popup.clone() else { return actions };
        let mut confirmed_idx = None;

        egui::Area::new(egui::Id::new("select_popup_overlay"))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                let screen_rect = ctx.content_rect();
                ui.painter().rect_filled(screen_rect, 0.0, Color32::from_black_alpha(180));

                egui::Frame::new()
                    .fill(Color32::from_rgb(20, 24, 33))
                    .stroke(egui::Stroke::new(1.5, Color32::from_rgb(56, 189, 248)))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(20, 16))
                    .show(ui, |ui| {
                        ui.set_width(300.0);
                        ui.vertical_centered(|ui| {
                            ui.heading(RichText::new(popup.title).strong().size(17.0).color(Color32::WHITE));
                            ui.add_space(3.0);
                            ui.label(RichText::new("Choose an option").weak().size(12.0));
                            ui.add_space(6.0);
                            ui.separator();
                            ui.add_space(6.0);

                            for (i, opt) in popup.options.iter().enumerate() {
                                let is_highlighted = popup.selected == i;
                                let mut frame = egui::Frame::new()
                                    .corner_radius(egui::CornerRadius::same(8))
                                    .inner_margin(egui::Margin::symmetric(12, 7));
                                if is_highlighted {
                                    frame = frame
                                        .fill(Color32::from_rgb(37, 99, 235))
                                        .stroke(egui::Stroke::new(1.5, Color32::from_rgb(147, 197, 253)));
                                } else {
                                    frame = frame.fill(Color32::from_rgb(28, 33, 44));
                                }
                                let resp = frame.show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if is_highlighted {
                                            ui.label(RichText::new("▶").color(Color32::from_rgb(147, 197, 253)).strong());
                                        } else {
                                            ui.add_space(14.0);
                                        }
                                        ui.label(RichText::new(opt).size(13.5).strong().color(Color32::WHITE));
                                    });
                                });
                                ui.add_space(3.0);
                                if resp.response.clicked() {
                                    confirmed_idx = Some(i);
                                }
                            }

                            ui.add_space(6.0);
                            ui.separator();
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                let total_w = ui.available_width();
                                let hint_w = 210.0;
                                if total_w > hint_w {
                                    ui.add_space((total_w - hint_w) / 2.0);
                                }
                                button_hint(ui, "A", "Confirm", Color32::from_rgb(22, 163, 74));
                                ui.add_space(10.0);
                                button_hint(ui, "B", "Cancel", Color32::from_rgb(220, 38, 38));
                            });
                        });
                    });
            });

        if let Some(idx) = confirmed_idx {
            if let Some(p) = &mut self.select_popup {
                p.selected = idx;
            }
            if let Some(act) = self.confirm_select_popup(settings) {
                actions.push(act);
            }
        }
        actions
    }

    /// triggers the pause menu action at pause_selected index.
    pub fn trigger_pause_selected(&mut self, game_info: Option<(PathBuf, String, Option<u64>)>) -> Option<Action> {
        let has_game_settings = game_info.as_ref().and_then(|g| g.2).is_some();
        match self.pause_selected {
            0 => Some(Action::Resume),
            1 => {
                self.slot = if self.slot >= 9 { 1 } else { self.slot + 1 };
                None
            }
            2 => Some(Action::SaveState),
            3 => Some(Action::LoadState),
            4 if has_game_settings => {
                if let Some((_, name, Some(id))) = game_info {
                    self.open_game_settings(id, name);
                }
                None
            }
            4 => {
                self.cheats_open = true;
                self.modal_just_opened = true;
                None
            }
            5 if has_game_settings => {
                self.cheats_open = true;
                self.modal_just_opened = true;
                None
            }
            5 => Some(Action::Reset),
            6 if has_game_settings => Some(Action::Reset),
            6 => Some(Action::Library),
            7 if has_game_settings => Some(Action::Library),
            7 => Some(Action::Quit),
            8 if has_game_settings => Some(Action::Quit),
            _ => None,
        }
    }

    /// whether something is open that stops the game while it is.
    pub fn over_game(&self) -> bool {
        self.menu_open || self.modal_open()
    }

    /// the library, filling the window. waiting are the games waiting for
    /// their turn to recompile.
    pub fn library(&mut self, ui: &mut egui::Ui, library: &Library, settings: &Settings, jobs: &[Job], waiting: &[Wanted]) -> Vec<Action> {
        let mut actions = Vec::new();
        if library.changed {
            self.icons.clear();
        }
        egui::Panel::top("library bar").show(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.heading(RichText::new("🌸 Zakuro").strong().size(20.0));
                if !library.games.is_empty() {
                    pill_badge(ui, &format!("{} TITLES", library.games.len()), Color32::from_rgb(30, 42, 60), Color32::from_rgb(96, 165, 250));
                }
                ui.separator();
                match &settings.games {
                    Some(folder) => ui.label(RichText::new(folder.display().to_string()).weak()),
                    None => ui.label(RichText::new("no games folder yet").weak()),
                };
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Settings").clicked() {
                        self.open_settings();
                    }
                    if ui.button("Multiplayer").clicked() {
                        self.multiplayer_open = true;
                        self.modal_just_opened = true;
                    }
                    if ui.add_enabled(settings.games.is_some(), egui::Button::new("Refresh")).clicked() {
                        actions.push(Action::Rescan);
                    }
                    if ui.button("Choose folder…").clicked() {
                        actions.push(Action::ChooseFolder);
                    }
                    let outdated = idle(library.games.iter().filter(|game| game.outdated).map(Game::wanted), jobs, waiting);
                    if !outdated.is_empty() {
                        let recompile = ui.button(format!("Recompile outdated ({})", outdated.len())).on_hover_text(
                            "These games were recompiled by an older version of Zakuro. Recompiling them again gives them its improvements. They go one at a time, around ten minutes each, and games can be played meanwhile.",
                        );
                        if recompile.clicked() {
                            actions.push(Action::Recompile(outdated));
                        }
                    }
                    if !waiting.is_empty() && ui.button(format!("Cancel the {} waiting", waiting.len())).on_hover_text("The game recompiling now carries on.").clicked() {
                        actions.push(Action::CancelWaiting);
                    }
                });
            });
            ui.add_space(8.0);
        });
        egui::Panel::bottom("library controller bar").show(ui, |ui| {
            ui.add_space(5.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                button_hint(ui, "A", "Play", Color32::from_rgb(22, 163, 74));
                ui.add_space(10.0);
                button_hint(ui, "X", "Mods", Color32::from_rgb(37, 99, 235));
                ui.add_space(10.0);
                button_hint(ui, "Y", "Game Settings", Color32::from_rgb(202, 138, 4));
                ui.add_space(10.0);
                button_hint(ui, "Start", "Settings", Color32::from_rgb(75, 85, 99));
                ui.add_space(10.0);
                button_hint(ui, "Select", "Refresh", Color32::from_rgb(75, 85, 99));
                ui.add_space(10.0);
                button_hint(ui, "LT/RT", "Page", Color32::from_rgb(75, 85, 99));
                ui.add_space(10.0);
                button_hint(ui, "D-Pad / Stick", "Navigate", Color32::from_rgb(75, 85, 99));
            });
            ui.add_space(5.0);
        });
        let background = match self.background.get(ui.ctx(), settings.background.as_deref()) {
            Ok(texture) => texture.map(|texture| (texture.id(), texture.size())),
            Err(error) => {
                self.message = Some(error);
                None
            }
        };
        egui::CentralPanel::default().show(ui, |ui| {
            if let Some((texture, size)) = background {
                let area = ui.clip_rect();
                let alpha = (settings.background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
                ui.painter().image(texture, area, cover(area.size(), size), Color32::from_white_alpha(alpha));
            }
            if settings.games.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() / 3.0);
                    ui.label("Pick the folder your games are in to see them here.");
                    if ui.button("Choose folder…").clicked() {
                        actions.push(Action::ChooseFolder);
                    }
                });
                return;
            }
            if library.is_scanning() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Looking for games…");
                });
                return;
            }
            if library.games.is_empty() {
                ui.label("No .3ds, .cci, .cxi or .cia files in that folder.");
                return;
            }
            if !library.games.is_empty() {
                self.selected_game = self.selected_game.min(library.games.len() - 1);
            } else {
                self.selected_game = 0;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, game) in library.games.iter().enumerate() {
                    let job = jobs.iter().find(|job| job.program_id == game.program_id);
                    let is_selected = self.selected_game == i;

                    let mut frame = egui::Frame::group(ui.style())
                        .corner_radius(egui::CornerRadius::same(10));
                    if is_selected {
                        frame = frame
                            .fill(Color32::from_rgb(28, 38, 54))
                            .stroke(egui::Stroke::new(2.0, Color32::from_rgb(56, 189, 248)));
                    } else if background.is_some() {
                        frame = frame.fill(ui.visuals().panel_fill.gamma_multiply(0.85));
                    } else {
                        frame = frame
                            .fill(Color32::from_rgb(24, 27, 36))
                            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(44, 50, 66)));
                    }
                    let card_resp = frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let icon = game.icon.as_ref().map(|pixels| {
                                self.icons.entry(game.path.clone()).or_insert_with(|| {
                                    let image = egui::ColorImage::from_rgba_unmultiplied([ICON_SIZE, ICON_SIZE], pixels);
                                    ui.ctx().load_texture(game.path.display().to_string(), image, egui::TextureOptions::LINEAR)
                                })
                            });
                            match icon {
                                Some(texture) => {
                                    ui.add(egui::Image::new(&*texture).fit_to_exact_size(Vec2::splat(ICON_SIZE as f32)));
                                }
                                None => {
                                    ui.allocate_space(Vec2::splat(ICON_SIZE as f32));
                                }
                            }
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&game.name).strong().size(16.0));
                                if !game.publisher.is_empty() {
                                    ui.label(RichText::new(&game.publisher).weak().size(12.0));
                                }
                                if let Some(problem) = &game.problem {
                                    ui.label(RichText::new(problem).color(Color32::from_rgb(230, 160, 80)));
                                    return;
                                }
                                ui.horizontal_wrapped(|ui| {
                                    pill_badge(ui, &format!("{:016X}", game.program_id), Color32::from_rgb(32, 36, 48), Color32::from_rgb(145, 155, 175));
                                    if game.outdated {
                                        pill_badge(ui, "OLD RECOMP", Color32::from_rgb(55, 42, 18), Color32::from_rgb(251, 191, 36));
                                    } else if game.recompiled {
                                        pill_badge(ui, "⚡ RECOMPILED", Color32::from_rgb(16, 52, 30), Color32::from_rgb(52, 211, 153));
                                    }
                                    if game.modded {
                                        pill_badge(ui, "MODS", Color32::from_rgb(20, 48, 76), Color32::from_rgb(56, 189, 248));
                                    }
                                    if settings.has_own(game.program_id) {
                                        pill_badge(ui, "CUSTOM SETTINGS", Color32::from_rgb(34, 45, 62), Color32::from_rgb(96, 165, 250));
                                    }
                                    if let Some(update) = &game.update {
                                        let version = zakuro_fs::version_name(update.version);
                                        pill_badge(ui, &format!("UPDATE {version}"), Color32::from_rgb(52, 38, 20), Color32::from_rgb(245, 158, 11));
                                    }
                                    if !game.dlc.is_empty() {
                                        pill_badge(ui, "DLC", Color32::from_rgb(44, 28, 62), Color32::from_rgb(192, 132, 252));
                                    }
                                });
                            });
                            if game.problem.is_some() {
                                return;
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let play_btn = egui::Button::new(RichText::new("▶ Play").size(15.0).strong());
                                let play_resp = if is_selected {
                                    ui.add(play_btn.fill(Color32::from_rgb(22, 120, 60)).stroke(egui::Stroke::new(1.5, Color32::from_rgb(74, 222, 128))))
                                } else {
                                    ui.add(play_btn)
                                };
                                if play_resp.clicked() {
                                    actions.push(Action::Play(game.path.clone()));
                                }
                                match job {
                                    Some(job) if !job.stage().finished() => {
                                        if ui.button("Cancel").clicked() {
                                            actions.push(Action::CancelRecompile(job.program_id));
                                        }
                                        progress(ui, job);
                                    }
                                    _ if waiting.iter().any(|wanted| wanted.program_id == game.program_id) => {
                                        if ui.button("Cancel").clicked() {
                                            actions.push(Action::CancelRecompile(game.program_id));
                                        }
                                        ui.label(RichText::new("waiting to recompile").weak());
                                    }
                                    _ => {
                                        let label = if game.recompiled { "Recompile again" } else { "Recompile" };
                                        if ui.button(label).on_hover_text("Turn the game's code into native code with 3dsrecomp, which makes it run faster. It takes around ten minutes and needs a C compiler, gcc or clang, or Zig, which Zakuro offers to download when the computer has neither.").clicked() {
                                            actions.push(Action::Recompile(vec![game.wanted()]));
                                        }
                                        if let Some(Stage::Failed(error)) = job.map(Job::stage) {
                                            ui.label(RichText::new(format!("failed, {error}")).color(Color32::LIGHT_RED));
                                        }
                                    }
                                }
                                if ui.button("Mods").on_hover_text("Open the game's mods folder. Files put in its romfs folder replace the game's, the way Luma3DS and Citra take mods, and a texture pack made for Citra or Azahar goes in its textures folder.").clicked() {
                                    actions.push(Action::Mods(game.program_id));
                                }
                                if ui.button("Settings").on_hover_text("What this game has of its own, over the general settings.").clicked() {
                                    self.game_settings = Some((game.program_id, game.name.clone()));
                                }
                            });
                        });
                    });
                    if card_resp.response.clicked() {
                        self.selected_game = i;
                    }
                    if is_selected && self.scroll_to_selected {
                        self.scroll_to_selected = false;
                        card_resp.response.scroll_to_me(Some(egui::Align::Center));
                    }
                }
            });
        });
        actions
    }

    /// what goes over a running game, the menu when it is open and the
    /// frame rate when asked for.
    #[allow(clippy::too_many_arguments)]
    pub fn game(
        &mut self,
        ui: &mut egui::Ui,
        name: &str,
        program_id: Option<u64>,
        fps: Option<f32>,
        recompiled: bool,
        fast: bool,
        jobs: &[Job],
        waiting: usize,
        saved: Option<&str>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let ctx = ui.ctx().clone();
        if let Some((text, since)) = &self.toast {
            if since.elapsed() < std::time::Duration::from_millis(2500) {
                egui::Area::new(egui::Id::new("toast"))
                    .anchor(egui::Align2::CENTER_TOP, Vec2::new(0.0, 8.0))
                    .interactable(false)
                    .show(&ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(text);
                        });
                    });
                ctx.request_repaint();
            } else {
                self.toast = None;
            }
        }
        if fast {
            egui::Area::new(egui::Id::new("fast forward"))
                .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-8.0, 8.0))
                .interactable(false)
                .show(&ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.label(RichText::new("» fast forward").monospace());
                    });
                });
        }
        if let Some(fps) = fps {
            // clicks go through to the screen under it
            egui::Area::new(egui::Id::new("fps")).fixed_pos(egui::pos2(8.0, 8.0)).interactable(false).show(&ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.label(RichText::new(format!("{fps:.0} fps")).monospace());
                });
            });
        }
        if !self.menu_open || self.modal_open() {
            return actions;
        }
        egui::Area::new(egui::Id::new("pause_menu_modal"))
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(&ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(22, 25, 33))
                    .stroke(egui::Stroke::new(1.5, Color32::from_rgb(56, 189, 248)))
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::symmetric(16, 16))
                    .show(ui, |ui| {
                        let pause_w = 260.0;
                        ui.set_min_width(pause_w);
                        ui.set_max_width(pause_w);
                        let item_spacing = 8.0;
                        ui.spacing_mut().item_spacing = egui::vec2(item_spacing, 6.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(name).strong().size(18.0));
                            ui.add_space(2.0);
                            pill_badge(
                                ui,
                                if recompiled { "⚡ RECOMPILED" } else { "INTERPRETED" },
                                if recompiled { Color32::from_rgb(30, 42, 60) } else { Color32::from_rgb(45, 35, 25) },
                                if recompiled { Color32::from_rgb(96, 165, 250) } else { Color32::from_rgb(251, 191, 36) },
                            );
                            ui.add_space(8.0);

                            let pause_sel = self.pause_selected;
                            let mut new_pause_sel = pause_sel;

                            // 0. Resume
                            let (c, h) = nav_button(ui, pause_sel == 0, "Resume");
                            if h { new_pause_sel = 0; }
                            if c { actions.push(Action::Resume); }

                            ui.add_space(4.0);

                            // 1. Slot selector (Highlight when selected / hovered)
                            let arrow_w = 30.0;
                            let text_w = pause_w - (arrow_w * 2.0) - (item_spacing * 2.0);
                            let is_slot_sel = pause_sel == 1;
                            let mut slot_frame = egui::Frame::new()
                                .corner_radius(egui::CornerRadius::same(6))
                                .inner_margin(egui::Margin::symmetric(2, 2));
                            if is_slot_sel {
                                slot_frame = slot_frame
                                    .fill(Color32::from_rgb(30, 48, 75))
                                    .stroke(egui::Stroke::new(1.5, Color32::from_rgb(56, 189, 248)));
                            } else {
                                slot_frame = slot_frame.fill(Color32::TRANSPARENT);
                            }
                            let resp_slot = slot_frame.show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = item_spacing;
                                    ui.spacing_mut().button_padding = egui::vec2(4.0, 2.0);
                                    if ui.add_sized([arrow_w, 26.0], egui::Button::new("◀")).clicked() {
                                        self.slot = if self.slot <= 1 { 9 } else { self.slot - 1 };
                                    }
                                    let slot_label = format!("Slot {} ({})", self.slot, saved.unwrap_or("empty"));
                                    ui.allocate_ui_with_layout(egui::vec2(text_w, 26.0), Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                                        ui.label(RichText::new(slot_label).strong().size(13.0).color(Color32::from_rgb(220, 230, 242)));
                                    });
                                    if ui.add_sized([arrow_w, 28.0], egui::Button::new("▶")).clicked() {
                                        self.slot = if self.slot >= 9 { 1 } else { self.slot + 1 };
                                    }
                                });
                            }).response;
                            if resp_slot.hovered() { new_pause_sel = 1; }

                            // 2. Save state & 3. Load state
                            let half_w = (pause_w - item_spacing) / 2.0;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = item_spacing;
                                let focused_save = pause_sel == 2;
                                let btn_save = egui::Button::new(RichText::new("Save state").size(13.5));
                                let resp_save = if focused_save {
                                    ui.add_sized([half_w, 32.0], btn_save.fill(Color32::from_rgb(37, 99, 235)).stroke(egui::Stroke::new(1.5, Color32::from_rgb(147, 197, 253))))
                                } else {
                                    ui.add_sized([half_w, 32.0], btn_save)
                                };
                                if resp_save.hovered() { new_pause_sel = 2; }
                                if resp_save.on_hover_text("F5 saves to this slot while playing.").clicked() {
                                    actions.push(Action::SaveState);
                                }

                                let focused_load = pause_sel == 3;
                                let has_save = saved.is_some();
                                let btn_load = egui::Button::new(
                                    RichText::new("Load state")
                                        .size(13.5)
                                        .color(if has_save { Color32::WHITE } else { Color32::from_rgb(120, 130, 145) })
                                );
                                let resp_load = if focused_load {
                                    ui.add_sized([half_w, 32.0], btn_load.fill(Color32::from_rgb(37, 99, 235)).stroke(egui::Stroke::new(1.5, Color32::from_rgb(147, 197, 253))))
                                } else if has_save {
                                    ui.add_sized([half_w, 32.0], btn_load)
                                } else {
                                    ui.add_sized([half_w, 32.0], btn_load.fill(Color32::from_rgb(26, 29, 38)))
                                };
                                if resp_load.hovered() { new_pause_sel = 3; }
                                if has_save && resp_load.on_hover_text("F8 loads this slot while playing.").clicked() {
                                    actions.push(Action::LoadState);
                                }
                            });

                            ui.add_space(4.0);

                            // 4. Game Settings (ONLY Game Settings in pause menu)
                            let mut current_idx = 4;
                            if let Some(program_id) = program_id {
                                let (c, h) = nav_button(ui, pause_sel == current_idx, "Game Settings");
                                if h { new_pause_sel = current_idx; }
                                if c {
                                    self.open_game_settings(program_id, name.to_owned());
                                }
                                current_idx += 1;
                            }

                            // Cheats
                            let (c, h) = nav_button(ui, pause_sel == current_idx, "Cheats");
                            if h { new_pause_sel = current_idx; }
                            if c {
                                self.cheats_open = true;
                                self.modal_just_opened = true;
                            }
                            current_idx += 1;

                            // Reset
                            let (c, h) = nav_button(ui, pause_sel == current_idx, "Reset");
                            if h { new_pause_sel = current_idx; }
                            if c { actions.push(Action::Reset); }
                            current_idx += 1;

                            // Back to library
                            let (c, h) = nav_button(ui, pause_sel == current_idx, "Back to the library");
                            if h { new_pause_sel = current_idx; }
                            if c { actions.push(Action::Library); }
                            current_idx += 1;

                            // Quit
                            let (c, h) = nav_button(ui, pause_sel == current_idx, "Quit");
                            if h { new_pause_sel = current_idx; }
                            if c { actions.push(Action::Quit); }

                            for job in jobs.iter().filter(|job| !job.stage().finished()) {
                                ui.separator();
                                ui.label(&job.name);
                                progress(ui, job);
                            }
                            if waiting > 0 {
                                ui.label(RichText::new(format!("{waiting} more waiting to recompile")).weak());
                            }

                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                let hint_w = 216.0;
                                let pad = ((pause_w - hint_w) / 2.0).max(0.0);
                                ui.add_space(pad);
                                button_hint(ui, "A", "Select", Color32::from_rgb(22, 163, 74));
                                ui.add_space(6.0);
                                button_hint(ui, "B", "Resume", Color32::from_rgb(220, 38, 38));
                                ui.add_space(6.0);
                                button_hint(ui, "◀/▶", "Slot", Color32::from_rgb(50, 60, 75));
                            });
                            self.pause_selected = new_pause_sel;
                        });
                    });
                });
        actions
    }

    /// the cheats window over a game, when open, its cheats by name, whether
    /// each is on, and their notes.
    pub fn cheats(&mut self, ctx: &egui::Context, cheats: &[(String, bool, String, bool)]) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut open = self.cheats_open;
        egui::Window::new("Cheats")
            .open(&mut open)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(420.0)
            .show(ctx, |ui| {
            let mut checkbox = |ui: &mut egui::Ui, index: usize, name: &str, enabled: bool, notes: &str| {
                let mut on = enabled;
                let mut response = ui.checkbox(&mut on, name);
                if !notes.is_empty() {
                    response = response.on_hover_text(notes);
                }
                if response.changed() {
                    actions.push(Action::Cheat(index, on));
                }
            };
            // Zakuro's own, tested on this build of the game
            if cheats.iter().any(|cheat| cheat.3) {
                ui.label(RichText::new("Enhancements").strong());
                for (index, (name, enabled, notes, _)) in cheats.iter().enumerate().filter(|(_, cheat)| cheat.3) {
                    checkbox(ui, index, name, *enabled, "");
                    // what it does to the game and who made it, in sight
                    // before it is turned on
                    ui.indent(index, |ui| ui.label(RichText::new(notes).weak().small()));
                }
                ui.separator();
            }
            if !cheats.iter().any(|cheat| !cheat.3) {
                ui.label(RichText::new("No cheats yet. Add one below, or put the cheat file Citra or Azahar has for this game in the cheats folder.").weak());
            }
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for (index, (name, enabled, notes, _)) in cheats.iter().enumerate().filter(|(_, cheat)| !cheat.3) {
                    checkbox(ui, index, name, *enabled, notes);
                }
            });
            ui.separator();
            ui.label("Add a cheat");
            ui.add(egui::TextEdit::singleline(&mut self.cheat_name).hint_text("Name"));
            ui.add(egui::TextEdit::multiline(&mut self.cheat_code).hint_text("00000000 00000000").desired_rows(4).font(egui::TextStyle::Monospace));
            ui.horizontal(|ui| {
                if ui.button("Add").clicked() && !self.cheat_code.trim().is_empty() {
                    actions.push(Action::AddCheat(std::mem::take(&mut self.cheat_name), std::mem::take(&mut self.cheat_code)));
                }
                if ui.button("Reload").on_hover_text("Read the cheats again from their file, after changing it.").clicked() {
                    actions.push(Action::ReloadCheats);
                }
                if ui.button("Open the cheats folder").clicked() {
                    actions.push(Action::CheatsFolder);
                }
            });
        });
        self.cheats_open = open;
        actions
    }

    /// the multiplayer window, when open: the room the player is in, or
    /// joining one and opening one.
    pub fn multiplayer(&mut self, ctx: &egui::Context, settings: &mut Settings, room: Option<&RoomView>) -> Vec<Action> {
        let mut actions = Vec::new();
        let before = settings.clone();
        let mut open = self.multiplayer_open;
        egui::Window::new("Multiplayer")
            .open(&mut open)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(420.0)
            .show(ctx, |ui| {
            ui.label(
                RichText::new("Local wireless play with others goes through a room, which one of you opens and the others join over the network.")
                    .weak(),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Nickname");
                ui.add(egui::TextEdit::singleline(&mut settings.nickname).char_limit(10).hint_text("Player"))
                    .on_hover_text("The name others see, and the console's own, which games show. It changes with the next game started.");
            });
            ui.separator();
            match room {
                Some(room) => {
                    ui.label(RichText::new(format!("In the room {}", room.name)).strong());
                    if let Some(port) = room.port {
                        ui.label(format!("Open on port {port}. Others join with this computer's address and that port. Over the internet the router has to let that port through."));
                    }
                    ui.add_space(4.0);
                    for (nickname, game) in &room.members {
                        match game.is_empty() {
                            true => ui.label(nickname),
                            false => ui.label(format!("{nickname}, playing {game}")),
                        };
                    }
                    ui.add_space(4.0);
                    let leave = if room.port.is_some() { "Close the room" } else { "Leave the room" };
                    if ui.button(leave).clicked() {
                        actions.push(Action::LeaveRoom);
                    }
                    if let Some(download_play) = &room.download_play {
                        ui.separator();
                        ui.label(RichText::new("Download Play").strong());
                        match &download_play.doing {
                            Some((doing, progress)) => {
                                ui.label(doing);
                                if let Some(progress) = progress {
                                    ui.add(egui::ProgressBar::new(*progress).show_percentage());
                                }
                                if ui.button("Leave").clicked() {
                                    actions.push(Action::DlpLeave);
                                }
                            }
                            None if download_play.offers.is_empty() => {
                                ui.label(RichText::new("No one in the room offers a game to download. A game offers one from its own Download Play menu.").weak());
                            }
                            None => {
                                for (index, offer) in download_play.offers.iter().enumerate() {
                                    ui.horizontal(|ui| {
                                        if let Some(pixels) = &offer.icon {
                                            let texture = self.dlp_icons.entry(offer.child).or_insert_with(|| {
                                                let image = egui::ColorImage::from_rgba_unmultiplied([ICON_SIZE, ICON_SIZE], pixels);
                                                ui.ctx().load_texture(format!("download play {:016X}", offer.child), image, egui::TextureOptions::LINEAR)
                                            });
                                            ui.add(egui::Image::new(&*texture).fit_to_exact_size(Vec2::splat(32.0)));
                                        }
                                        ui.vertical(|ui| {
                                            ui.label(RichText::new(&offer.title).strong());
                                            ui.label(RichText::new(format!("from {}, {} of {} joined", offer.host, offer.guests, offer.most)).weak());
                                        });
                                        if ui.add_enabled(offer.blocked.is_none(), egui::Button::new("Join")).clicked() {
                                            actions.push(Action::DlpJoin(index));
                                        }
                                    });
                                    if let Some(why) = &offer.blocked {
                                        ui.label(RichText::new(why).weak());
                                    }
                                }
                            }
                        }
                    }
                }
                None => {
                    ui.label(RichText::new("Join a room").strong());
                    ui.horizontal(|ui| {
                        ui.label("Address");
                        ui.add(egui::TextEdit::singleline(&mut settings.room_address).hint_text("192.168.0.10:24873"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Password");
                        ui.add(egui::TextEdit::singleline(&mut self.room_password).password(true).hint_text("none"));
                    });
                    if ui.add_enabled(!settings.room_address.trim().is_empty(), egui::Button::new("Join")).clicked() {
                        actions.push(Action::JoinRoom);
                    }
                    ui.separator();
                    ui.label(RichText::new("Open a room").strong());
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut settings.room_name).hint_text("Zakuro"));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Port");
                        ui.add(egui::DragValue::new(&mut settings.room_port).range(1024..=65535));
                    });
                    ui.label(RichText::new("The password above, when there is one, is the room's.").weak());
                    if ui.button("Open").clicked() {
                        actions.push(Action::HostRoom);
                    }
                }
            }
        });
        self.multiplayer_open = open;
        if *settings != before {
            actions.push(Action::Settings);
        }
        actions
    }

    /// the settings window, when open. own says the running game has
    /// settings of its own, which go over these.
    pub fn settings(&mut self, ctx: &egui::Context, settings: &mut Settings, own: bool) -> Vec<Action> {
        let mut actions = Vec::new();
        let before = settings.clone();
        let mut open = self.settings_open;
        let max_h = (ctx.content_rect().height() - 24.0).max(200.0);
        let win_h = 480.0f32.min(max_h);
        let scroll_h = (win_h - 150.0).max(60.0);
        egui::Window::new("⚙ Settings")
            .id(egui::Id::new("settings"))
            .open(&mut open)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(580.0)
            .max_width(580.0)
            .max_height(max_h)
            .show(ctx, |ui| {
                if own {
                    ui.label(RichText::new("This game has settings of its own, which go over these. They are in Settings for this game, in the menu.").weak());
                }

                // Tab Bar
                ui.horizontal(|ui| {
                    button_hint(ui, "LB", "", Color32::from_rgb(50, 60, 75));
                    for tab in SettingsTab::ALL {
                        let is_current = self.settings_tab == tab;
                        let btn = egui::Button::new(RichText::new(tab.name()).strong().size(13.0));
                        let resp = if is_current {
                            ui.add_sized([105.0, 30.0], btn.fill(Color32::from_rgb(37, 99, 235)).stroke(egui::Stroke::new(1.5, Color32::from_rgb(147, 197, 253))))
                        } else {
                            ui.add_sized([105.0, 30.0], btn)
                        };
                        if resp.clicked() {
                            self.settings_tab = tab;
                            self.settings_selected = 0;
                            self.settings_scroll_to_selected = true;
                        }
                    }
                    button_hint(ui, "RB", "", Color32::from_rgb(50, 60, 75));
                });
                ui.add_space(4.0);
                ui.separator();

                let scroll = self.settings_scroll_to_selected;

                egui::ScrollArea::vertical().max_height(scroll_h).show(ui, |ui| {
                    match self.settings_tab {
                        SettingsTab::Graphics => {
                            let sel = self.settings_selected;
                            let mut new_sel = sel;
                            if setting_row(ui, sel == 0, scroll, |ui| {
                                egui::ComboBox::from_label("Presented with")
                                    .selected_text(match settings.renderer {
                                        Renderer::Vulkan => "Vulkan",
                                        Renderer::OpenGl => "OpenGL",
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut settings.renderer, Renderer::Vulkan, "Vulkan");
                                        ui.selectable_value(&mut settings.renderer, Renderer::OpenGl, "OpenGL");
                                    });
                            }) { new_sel = 0; }
                            if setting_row(ui, sel == 1, scroll, |ui| rasterizer(ui, &mut settings.hardware_rasterizer)) { new_sel = 1; }
                            let hw = settings.hardware_rasterizer;
                            if setting_row(ui, sel == 2, scroll, |ui| {
                                ui.add_enabled_ui(hw, |ui| resolution(ui, &mut settings.resolution));
                            }) { new_sel = 2; }
                            if setting_row(ui, sel == 3, scroll, |ui| {
                                ui.add_enabled_ui(hw, |ui| texture_packs(ui, &mut settings.texture_packs));
                            }) { new_sel = 3; }
                            let interpolates = hw && settings.renderer == Renderer::Vulkan;
                            if setting_row(ui, sel == 4, scroll, |ui| {
                                ui.add_enabled_ui(interpolates, |ui| interpolation(ui, &mut settings.frame_interpolation));
                            }) { new_sel = 4; }
                            if setting_row(ui, sel == 5, scroll, |ui| screens(ui, &mut settings.layout)) { new_sel = 5; }
                            if setting_row(ui, sel == 6, scroll, |ui| rotation(ui, &mut settings.orientation)) { new_sel = 6; }
                            if setting_row(ui, sel == 7, scroll, |ui| filter(ui, &mut settings.filter)) { new_sel = 7; }
                            if setting_row(ui, sel == 8, scroll, |ui| integer_scale(ui, &mut settings.integer_scale)) { new_sel = 8; }
                            if setting_row(ui, sel == 9, scroll, |ui| top_shape(ui, &mut settings.top_shape)) { new_sel = 9; }
                            if setting_row(ui, sel == 10, scroll, |ui| stereo_3d(ui, &mut settings.stereo)) { new_sel = 10; }
                            let stereo = settings.stereo != Stereo3d::Off;
                            if setting_row(ui, sel == 11, scroll, |ui| {
                                ui.add_enabled_ui(stereo, |ui| depth_3d(ui, &mut settings.depth_3d));
                            }) { new_sel = 11; }
                            if setting_row(ui, sel == 12, scroll, |ui| {
                                ui.checkbox(&mut settings.swap_eyes, "Swap eyes")
                                    .on_hover_text("Each eye gets the other's picture, for viewing cross-eyed, glasses worn the other way round, or a monitor whose rows start with the other eye.");
                            }) { new_sel = 12; }
                            ui.label(RichText::new("F9 switches them while playing, F10 between the top and the bottom screen alone.").weak());
                            ui.label(RichText::new("The presenter changes the next time Zakuro starts, the 3D with the next game.").weak());
                            self.settings_selected = new_sel;
                        }
                        SettingsTab::Controller => {
                            let sel = self.settings_selected;
                            let mut new_sel = sel;
                            ui.heading("Gamepad / Handheld Controls");
                            ui.label(RichText::new("Press (A) to rebind a button, (B) to cancel rebinding.").size(12.0).weak());
                            ui.add_space(6.0);

                            for (i, (name, button)) in settings.pad.all_mut().into_iter().enumerate() {
                                let is_sel = sel == i;
                                let is_rebinding = self.rebinding_pad == Some(i);
                                if setting_row(ui, is_sel, scroll, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(name).strong().size(13.0));
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            let text = if is_rebinding {
                                                "press button… (B cancels)".to_owned()
                                            } else {
                                                button_name(*button).to_owned()
                                            };
                                            let mut btn = egui::Button::new(RichText::new(text).size(12.5));
                                            if is_rebinding {
                                                btn = btn.fill(Color32::from_rgb(180, 83, 9)).stroke(egui::Stroke::new(1.5, Color32::from_rgb(251, 191, 36)));
                                            } else {
                                                btn = btn.fill(Color32::from_rgb(28, 38, 54));
                                            }
                                            if ui.add_sized([180.0, 26.0], btn).clicked() {
                                                self.rebinding_pad = Some(i);
                                                self.rebinding = None;
                                            }
                                        });
                                    });
                                }) {
                                    new_sel = i;
                                }
                            }
                            ui.add_space(4.0);
                            ui.label(RichText::new("The circle pad is the left stick, and Home opens the menu over the game.").weak());
                            if setting_row(ui, sel == 15, scroll, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Reset Gamepad Bindings").strong().size(13.0));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.add_sized([180.0, 26.0], egui::Button::new("Default controller")).clicked() {
                                            settings.pad = PadButtons::default();
                                            self.rebinding_pad = None;
                                        }
                                    });
                                });
                            }) { new_sel = 15; }

                            ui.add_space(8.0);
                            ui.separator();
                            ui.heading("Keyboard Controls");
                            egui::Grid::new("controls").num_columns(4).spacing([16.0, 6.0]).show(ui, |ui| {
                                for (i, (name, key)) in settings.keys.all_mut().into_iter().enumerate() {
                                    ui.label(name);
                                    let text = if self.rebinding == Some(i) { "press a key…".to_owned() } else { key_name(*key) };
                                    if ui.add_sized([120.0, 24.0], egui::Button::new(text)).clicked() {
                                        self.rebinding = Some(i);
                                        self.rebinding_pad = None;
                                    }
                                    if i % 2 == 1 {
                                        ui.end_row();
                                    }
                                }
                            });
                            if setting_row(ui, sel == 16, scroll, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Reset Keyboard Bindings").strong().size(13.0));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.add_sized([180.0, 26.0], egui::Button::new("Default keyboard")).clicked() {
                                            settings.keys = Keys::default();
                                            self.rebinding = None;
                                        }
                                    });
                                });
                            }) { new_sel = 16; }
                            ui.label(RichText::new("Drag with the right mouse button to tilt the console, for games that use the motion sensors.").weak());
                            self.settings_selected = new_sel;
                        }
                        SettingsTab::Audio => {
                            let sel = self.settings_selected;
                            let mut new_sel = sel;
                            ui.heading("Volume");
                            if setting_row(ui, sel == 0, scroll, |ui| {
                                ui.add(egui::Slider::new(&mut settings.volume, 0.0..=1.0).text("Volume").show_value(false));
                            }) { new_sel = 0; }
                            if setting_row(ui, sel == 1, scroll, |ui| {
                                ui.checkbox(&mut settings.mute, "Mute");
                            }) { new_sel = 1; }

                            ui.add_space(8.0);
                            ui.separator();
                            ui.heading("Microphone");
                            if setting_row(ui, sel == 2, scroll, |ui| {
                                egui::ComboBox::from_label("Microphone").selected_text(settings.microphone.name()).show_ui(ui, |ui| {
                                    for source in MicrophoneSource::ALL {
                                        ui.selectable_value(&mut settings.microphone, source, source.name());
                                    }
                                })
                                .response
                                .on_hover_text("What the console's microphone hears. The computer's microphone is open only while a game listens, which some do all the time, Super Mario 3D Land among them. Holding M blows into it either way, for games that ask you to.");
                            }) { new_sel = 2; }
                            self.settings_selected = new_sel;
                        }
                        SettingsTab::System => {
                            let sel = self.settings_selected;
                            let mut new_sel = sel;
                            ui.heading("Display & Window");
                            if setting_row(ui, sel == 0, scroll, |ui| {
                                egui::ComboBox::from_label("Window mode")
                                    .selected_text(settings.window_mode.name())
                                    .show_ui(ui, |ui| {
                                        for mode in WindowMode::ALL {
                                            ui.selectable_value(&mut settings.window_mode, mode, mode.name());
                                        }
                                    });
                            }) { new_sel = 0; }
                            if setting_row(ui, sel == 1, scroll, |ui| window_scale(ui, &mut settings.scale)) { new_sel = 1; }
                            if setting_row(ui, sel == 2, scroll, |ui| {
                                ui.checkbox(&mut settings.show_fps, "Show the frame rate");
                            }) { new_sel = 2; }
                            if setting_row(ui, sel == 3, scroll, |ui| {
                                if ui.button("Toggle Fullscreen (F11)").clicked() {
                                    actions.push(Action::Fullscreen);
                                }
                            }) { new_sel = 3; }

                            ui.add_space(8.0);
                            ui.separator();
                            ui.heading("Emulation");
                            if setting_row(ui, sel == 4, scroll, |ui| language(ui, &mut settings.language)) { new_sel = 4; }
                            if setting_row(ui, sel == 5, scroll, |ui| {
                                code(ui, &mut settings.recompiled);
                                ui.label(RichText::new("Recompiled code changes with the next game started, or Reset.").weak());
                            }) { new_sel = 5; }
                            if setting_row(ui, sel == 6, scroll, |ui| {
                                egui::ComboBox::from_label("Cameras").selected_text(settings.camera.name()).show_ui(ui, |ui| {
                                    for source in CameraSource::ALL {
                                        ui.selectable_value(&mut settings.camera, source, source.name());
                                    }
                                })
                                .response
                                .on_hover_text("What the console's cameras see. The computer's camera turns on only while a game takes pictures.");
                            }) { new_sel = 6; }
                            if setting_row(ui, sel == 7, scroll, |ui| {
                                ui.horizontal(|ui| {
                                    if ui.button("Show camera picture…").on_hover_text("Pick a picture for the console's cameras to see, a QR code's say, in place of the computer's camera.").clicked() {
                                        actions.push(Action::ChooseCameraPicture);
                                    }
                                    if settings.camera == CameraSource::Picture {
                                        match &settings.camera_picture {
                                            Some(path) => ui.label(path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()),
                                            None => ui.label(RichText::new("none").weak()),
                                        };
                                        if ui.button("Choose…").clicked() {
                                            actions.push(Action::ChooseCameraPicture);
                                        }
                                    }
                                });
                            }) { new_sel = 7; }
                            if setting_row(ui, sel == 8, scroll, |ui| {
                                if ui.button("Copy report info").clicked() {
                                    actions.push(Action::CopyReport);
                                }
                            }) { new_sel = 8; }

                            ui.add_space(8.0);
                            ui.separator();
                            ui.heading("Games & Files");
                            ui.horizontal(|ui| {
                                match &settings.games {
                                    Some(folder) => ui.label(folder.display().to_string()),
                                    None => ui.label(RichText::new("none").weak()),
                                };
                                if ui.button("Choose folder…").clicked() {
                                    actions.push(Action::ChooseFolder);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label("Library background");
                                match &settings.background {
                                    Some(path) => ui.label(path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()),
                                    None => ui.label(RichText::new("none").weak()),
                                };
                                if ui.button("Choose…").clicked() {
                                    actions.push(Action::ChooseBackground);
                                }
                                if settings.background.is_some() && ui.button("Remove").clicked() {
                                    settings.background = None;
                                }
                            });
                            if setting_row(ui, sel == 9, scroll, |ui| {
                                ui.add_enabled(
                                    settings.background.is_some(),
                                    egui::Slider::new(&mut settings.background_opacity, 0.0..=1.0)
                                        .text("Opacity")
                                        .custom_formatter(|value, _| format!("{:.0}%", value * 100.0)),
                                );
                            }) { new_sel = 9; }
                            self.settings_selected = new_sel;
                        }
                    }
                });
                self.settings_scroll_to_selected = false;

                ui.add_space(6.0);
                ui.separator();
                ui.horizontal(|ui| {
                    button_hint(ui, "LB / RB", "Switch Tab", Color32::from_rgb(50, 60, 75));
                    ui.add_space(8.0);
                    button_hint(ui, "D-Pad", "Navigate", Color32::from_rgb(75, 85, 99));
                    ui.add_space(8.0);
                    button_hint(ui, "A", "Select / Toggle", Color32::from_rgb(22, 163, 74));
                    ui.add_space(8.0);
                    button_hint(ui, "B", "Close", Color32::from_rgb(220, 38, 38));
                });
            });
        self.settings_open = open;
        if !open {
            self.rebinding = None;
            self.rebinding_pad = None;
        }
        if *settings != before {
            actions.push(Action::Settings);
        }
        actions
    }

    /// the window of what a game has of its own, when open.
    pub fn game_settings(&mut self, ctx: &egui::Context, settings: &mut Settings) -> Vec<Action> {
        let mut actions = Vec::new();
        let Some((program_id, name)) = self.game_settings.clone() else { return actions };
        let key = game_key(program_id);
        let mut game = settings.per_game.get(&key).cloned().unwrap_or_default();
        let before = game.clone();
        let general: &Settings = settings;
        let mut open = true;
        let max_h = (ctx.content_rect().height() - 24.0).max(200.0);
        let win_h = 480.0f32.min(max_h);
        let scroll_h = (win_h - 150.0).max(60.0);

        egui::Window::new(format!("⚙ Settings: {name}"))
            .id(egui::Id::new("game settings"))
            .open(&mut open)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .default_width(560.0)
            .max_width(560.0)
            .max_height(max_h)
            .show(ctx, |ui| {
                ui.label(RichText::new("Customize settings for this game. Non-overridden options automatically follow Global Settings.").size(12.5).weak());
                ui.add_space(4.0);

                // Tab Bar
                ui.horizontal(|ui| {
                    button_hint(ui, "LB", "", Color32::from_rgb(50, 60, 75));
                    for tab in GameSettingsTab::ALL {
                        let is_current = self.game_settings_tab == tab;
                        let btn = egui::Button::new(RichText::new(tab.name()).strong().size(13.0));
                        let resp = if is_current {
                            ui.add_sized([120.0, 30.0], btn.fill(Color32::from_rgb(37, 99, 235)).stroke(egui::Stroke::new(1.5, Color32::from_rgb(147, 197, 253))))
                        } else {
                            ui.add_sized([120.0, 30.0], btn)
                        };
                        if resp.clicked() {
                            self.game_settings_tab = tab;
                            self.game_settings_selected = 0;
                            self.game_settings_scroll_to_selected = true;
                        }
                    }
                    button_hint(ui, "RB", "", Color32::from_rgb(50, 60, 75));
                });
                ui.add_space(4.0);
                ui.separator();

                let scroll = self.game_settings_scroll_to_selected;

                egui::ScrollArea::vertical().max_height(scroll_h).show(ui, |ui| {
                    match self.game_settings_tab {
                        GameSettingsTab::Graphics => {
                            let sel = self.game_settings_selected;
                            let mut new_sel = sel;

                            // 0: Hardware rasterizer
                            let hw_val = game.hardware_rasterizer.unwrap_or(general.hardware_rasterizer);
                            let (act, hov) = game_setting_row(ui, sel == 0, scroll, "Draw 3D on GPU", game.hardware_rasterizer.is_some(), if hw_val { "Enabled" } else { "Disabled" }, if general.hardware_rasterizer { "Enabled" } else { "Disabled" });
                            if hov { new_sel = 0; }
                            match act {
                                GameRowAction::Reset => game.hardware_rasterizer = None,
                                GameRowAction::Edit => game.hardware_rasterizer = Some(!hw_val),
                                GameRowAction::None => {}
                            }

                            // 1: Resolution
                            let res_val = game.resolution.unwrap_or(general.resolution);
                            let (act, hov) = game_setting_row(ui, sel == 1, scroll, "Internal Resolution", game.resolution.is_some(), &format!("{res_val}x"), &format!("{}x", general.resolution));
                            if hov { new_sel = 1; }
                            match act {
                                GameRowAction::Reset => game.resolution = None,
                                GameRowAction::Edit => game.resolution = Some(if res_val >= 8 { 1 } else { res_val + 1 }),
                                GameRowAction::None => {}
                            }

                            // 2: Custom Textures
                            let tex_val = game.texture_packs.unwrap_or(general.texture_packs);
                            let (act, hov) = game_setting_row(ui, sel == 2, scroll, "Custom HD Textures", game.texture_packs.is_some(), if tex_val { "Enabled" } else { "Disabled" }, if general.texture_packs { "Enabled" } else { "Disabled" });
                            if hov { new_sel = 2; }
                            match act {
                                GameRowAction::Reset => game.texture_packs = None,
                                GameRowAction::Edit => game.texture_packs = Some(!tex_val),
                                GameRowAction::None => {}
                            }

                            // 3: Frame interpolation
                            let interp_val = game.frame_interpolation.unwrap_or(general.frame_interpolation);
                            let (act, hov) = game_setting_row(ui, sel == 3, scroll, "Frame Interpolation", game.frame_interpolation.is_some(), if interp_val { "Enabled" } else { "Disabled" }, if general.frame_interpolation { "Enabled" } else { "Disabled" });
                            if hov { new_sel = 3; }
                            match act {
                                GameRowAction::Reset => game.frame_interpolation = None,
                                GameRowAction::Edit => game.frame_interpolation = Some(!interp_val),
                                GameRowAction::None => {}
                            }

                            // 4: Screen Layout
                            let layout_val = game.layout.unwrap_or(general.layout);
                            let (act, hov) = game_setting_row(ui, sel == 4, scroll, "Screen Layout", game.layout.is_some(), layout_val.name(), general.layout.name());
                            if hov { new_sel = 4; }
                            match act {
                                GameRowAction::Reset => game.layout = None,
                                GameRowAction::Edit => {
                                    self.select_popup = Some(SelectPopup::game(
                                        SelectTarget::Layout,
                                        program_id,
                                        "Screen Layout",
                                        Screens::ALL.iter().map(|s| s.name().to_string()).collect(),
                                        Screens::ALL.iter().position(|&s| s == layout_val).unwrap_or(0),
                                    ));
                                }
                                GameRowAction::None => {}
                            }

                            // 5: Screen Orientation
                            let ori_val = game.orientation.unwrap_or(general.orientation);
                            let (act, hov) = game_setting_row(ui, sel == 5, scroll, "Screen Orientation", game.orientation.is_some(), ori_val.name(), general.orientation.name());
                            if hov { new_sel = 5; }
                            match act {
                                GameRowAction::Reset => game.orientation = None,
                                GameRowAction::Edit => {
                                    self.select_popup = Some(SelectPopup::game(
                                        SelectTarget::Orientation,
                                        program_id,
                                        "Screen Orientation",
                                        Orientation::ALL.iter().map(|o| o.name().to_string()).collect(),
                                        Orientation::ALL.iter().position(|&o| o == ori_val).unwrap_or(0),
                                    ));
                                }
                                GameRowAction::None => {}
                            }

                            // 6: Texture Filter
                            let filt_val = game.filter.unwrap_or(general.filter);
                            let (act, hov) = game_setting_row(ui, sel == 6, scroll, "Texture Filter", game.filter.is_some(), filt_val.name(), general.filter.name());
                            if hov { new_sel = 6; }
                            match act {
                                GameRowAction::Reset => game.filter = None,
                                GameRowAction::Edit => {
                                    self.select_popup = Some(SelectPopup::game(
                                        SelectTarget::Filter,
                                        program_id,
                                        "Texture Filter",
                                        Filter::ALL.iter().map(|f| f.name().to_string()).collect(),
                                        Filter::ALL.iter().position(|&f| f == filt_val).unwrap_or(0),
                                    ));
                                }
                                GameRowAction::None => {}
                            }

                            // 7: Integer Scaling
                            let int_val = game.integer_scale.unwrap_or(general.integer_scale);
                            let (act, hov) = game_setting_row(ui, sel == 7, scroll, "Integer Scaling", game.integer_scale.is_some(), if int_val { "Enabled" } else { "Disabled" }, if general.integer_scale { "Enabled" } else { "Disabled" });
                            if hov { new_sel = 7; }
                            match act {
                                GameRowAction::Reset => game.integer_scale = None,
                                GameRowAction::Edit => game.integer_scale = Some(!int_val),
                                GameRowAction::None => {}
                            }

                            // 8: Top Screen Shape
                            let shape_val = game.top_shape.unwrap_or(general.top_shape);
                            let (act, hov) = game_setting_row(ui, sel == 8, scroll, "Top Screen Aspect", game.top_shape.is_some(), shape_val.name(), general.top_shape.name());
                            if hov { new_sel = 8; }
                            match act {
                                GameRowAction::Reset => game.top_shape = None,
                                GameRowAction::Edit => {
                                    self.select_popup = Some(SelectPopup::game(
                                        SelectTarget::TopShape,
                                        program_id,
                                        "Top Screen Aspect Ratio",
                                        TopShape::ALL.iter().map(|s| s.name().to_string()).collect(),
                                        TopShape::ALL.iter().position(|&s| s == shape_val).unwrap_or(0),
                                    ));
                                }
                                GameRowAction::None => {}
                            }

                            // 9: 3D Mode
                            let st_val = game.stereo.unwrap_or(general.stereo);
                            let (act, hov) = game_setting_row(ui, sel == 9, scroll, "Stereo 3D", game.stereo.is_some(), st_val.name(), general.stereo.name());
                            if hov { new_sel = 9; }
                            match act {
                                GameRowAction::Reset => game.stereo = None,
                                GameRowAction::Edit => {
                                    self.select_popup = Some(SelectPopup::game(
                                        SelectTarget::Stereo,
                                        program_id,
                                        "Stereo 3D Mode",
                                        Stereo3d::ALL.iter().map(|s| s.name().to_string()).collect(),
                                        Stereo3d::ALL.iter().position(|&s| s == st_val).unwrap_or(0),
                                    ));
                                }
                                GameRowAction::None => {}
                            }

                            // 10: 3D Depth
                            let dp_val = game.depth_3d.unwrap_or(general.depth_3d);
                            let (act, hov) = game_setting_row(ui, sel == 10, scroll, "3D Depth", game.depth_3d.is_some(), &format!("{dp_val}%"), &format!("{}%", general.depth_3d));
                            if hov { new_sel = 10; }
                            match act {
                                GameRowAction::Reset => game.depth_3d = None,
                                GameRowAction::Edit => game.depth_3d = Some(if dp_val >= 100 { 0 } else { dp_val + 10 }),
                                GameRowAction::None => {}
                            }

                            // 11: Reset all to Global Settings
                            let has_custom = game != GameSettings::default();
                            let is_reset_sel = sel == 11;
                            if setting_row(ui, is_reset_sel, scroll, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Reset All to Global Settings").strong().size(13.0).color(if has_custom { Color32::WHITE } else { Color32::from_rgb(120, 130, 145) }));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.add_enabled(has_custom, egui::Button::new("Follow Global Settings in Everything")).clicked() {
                                            game = GameSettings::default();
                                        }
                                    });
                                });
                            }) {
                                new_sel = 11;
                            }

                            self.game_settings_selected = new_sel;
                        }
                        GameSettingsTab::System => {
                            let sel = self.game_settings_selected;
                            let mut new_sel = sel;

                            // 0: Show Frame Rate
                            let fps_val = game.show_fps.unwrap_or(general.show_fps);
                            let (act, hov) = game_setting_row(
                                ui,
                                sel == 0,
                                scroll,
                                "Show Frame Rate",
                                game.show_fps.is_some(),
                                if fps_val { "Enabled" } else { "Disabled" },
                                if general.show_fps { "Enabled" } else { "Disabled" },
                            );
                            if hov { new_sel = 0; }
                            match act {
                                GameRowAction::Reset => game.show_fps = None,
                                GameRowAction::Edit => game.show_fps = Some(!fps_val),
                                GameRowAction::None => {}
                            }

                            // 1: Toggle Fullscreen
                            let is_full_sel = sel == 1;
                            if setting_row(ui, is_full_sel, scroll, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Toggle Fullscreen").strong().size(13.0).color(Color32::from_rgb(225, 232, 245)));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.button("Toggle Fullscreen (F11)").clicked() {
                                            actions.push(Action::Fullscreen);
                                        }
                                    });
                                });
                            }) {
                                new_sel = 1;
                            }

                            // 2: Window Mode
                            let win_val = game.window_mode.unwrap_or(general.window_mode);
                            let (act, hov) = game_setting_row(
                                ui,
                                sel == 2,
                                scroll,
                                "Window Mode",
                                game.window_mode.is_some(),
                                win_val.name(),
                                general.window_mode.name(),
                            );
                            if hov { new_sel = 2; }
                            match act {
                                GameRowAction::Reset => game.window_mode = None,
                                GameRowAction::Edit => {
                                    self.select_popup = Some(SelectPopup::game(
                                        SelectTarget::WindowMode,
                                        program_id,
                                        "Window Mode",
                                        WindowMode::ALL.iter().map(|w| w.name().to_string()).collect(),
                                        WindowMode::ALL.iter().position(|&w| w == win_val).unwrap_or(0),
                                    ));
                                }
                                GameRowAction::None => {}
                            }

                            // 3: Language
                            let lang_val = game.language.unwrap_or(general.language);
                            let (act, hov) = game_setting_row(ui, sel == 3, scroll, "Console Language", game.language.is_some(), lang_val.name(), general.language.name());
                            if hov { new_sel = 3; }
                            match act {
                                GameRowAction::Reset => game.language = None,
                                GameRowAction::Edit => {
                                    self.select_popup = Some(SelectPopup::game(
                                        SelectTarget::Language,
                                        program_id,
                                        "Console Language",
                                        Language::ALL.iter().map(|l| l.name().to_string()).collect(),
                                        Language::ALL.iter().position(|&l| l == lang_val).unwrap_or(0),
                                    ));
                                }
                                GameRowAction::None => {}
                            }

                            // 4: Recompiled code
                            let recomp_val = game.recompiled.unwrap_or(general.recompiled);
                            let (act, hov) = game_setting_row(ui, sel == 4, scroll, "Run Recompiled Code", game.recompiled.is_some(), if recomp_val { "Enabled" } else { "Disabled" }, if general.recompiled { "Enabled" } else { "Disabled" });
                            if hov { new_sel = 4; }
                            match act {
                                GameRowAction::Reset => game.recompiled = None,
                                GameRowAction::Edit => game.recompiled = Some(!recomp_val),
                                GameRowAction::None => {}
                            }

                            // 5: Reset all to Global Settings
                            let has_custom = game != GameSettings::default();
                            let is_reset_sel = sel == 5;
                            if setting_row(ui, is_reset_sel, scroll, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("Reset All to Global Settings").strong().size(13.0).color(if has_custom { Color32::WHITE } else { Color32::from_rgb(120, 130, 145) }));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.add_enabled(has_custom, egui::Button::new("Follow Global Settings in Everything")).clicked() {
                                            game = GameSettings::default();
                                        }
                                    });
                                });
                            }) {
                                new_sel = 5;
                            }

                            self.game_settings_selected = new_sel;
                        }
                    }
                });

                self.game_settings_scroll_to_selected = false;

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    button_hint(ui, "A", "Change / Override", Color32::from_rgb(22, 163, 74));
                    ui.add_space(6.0);
                    button_hint(ui, "X", "Use Global", Color32::from_rgb(37, 99, 235));
                    ui.add_space(6.0);
                    button_hint(ui, "LB/RB", "Tabs", Color32::from_rgb(50, 60, 75));
                    ui.add_space(6.0);
                    button_hint(ui, "B", "Close", Color32::from_rgb(220, 38, 38));
                });
            });
        if !open {
            self.game_settings = None;
        }
        if game != before {
            match game == GameSettings::default() {
                true => drop(settings.per_game.remove(&key)),
                false => drop(settings.per_game.insert(key, game)),
            }
            actions.push(Action::Settings);
        }
        actions
    }

    /// a message box, when there is something to tell.
    /// the keyboard a game opened to have text typed in, it waits until a
    /// button closes it.
    pub fn keyboard(&mut self, ctx: &egui::Context, request: &Request) -> Option<Action> {
        let fresh = self.typing.as_ref().is_none_or(|(asked, _)| asked != request);
        if fresh {
            self.typing = Some((request.clone(), request.text.clone()));
        }
        let (_, text) = self.typing.as_mut()?;
        let confirm = request.buttons.len() - 1;
        let mut action = None;
        egui::Window::new("Keyboard")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                let field = ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(&request.hint)
                        .char_limit(request.max_length)
                        .desired_width(260.0),
                );
                if fresh {
                    field.request_focus();
                }
                let valid = request.accepts(text);
                ui.horizontal(|ui| {
                    for (button, label) in request.buttons.iter().enumerate() {
                        if ui.add_enabled(button != confirm || valid, egui::Button::new(label)).clicked() {
                            action = Some(Action::Keyboard(text.clone(), button));
                        }
                    }
                });
                // enter confirms, when the text will do
                if valid && field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    action = Some(Action::Keyboard(text.clone(), confirm));
                }
            });
        if action.is_some() {
            self.typing = None;
        }
        action
    }

    /// asks whether to download a compiler, there being none to recompile
    /// with.
    pub fn compiler_offer(&mut self, ctx: &egui::Context) -> Option<Action> {
        let games = self.compiler_offer.clone()?;
        let size = crate::zig::download_size().map_or_else(String::new, |size| format!(", {} MB to download and around 400 MB once unpacked", size / 1_000_000));
        let mut action = None;
        egui::Window::new("Recompile")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Recompiling needs a C compiler, and Zakuro didn't find one on this computer. It can download Zig, a compiler that needs no installing, into its own folder{size}."
                ));
                ui.horizontal(|ui| {
                    if ui.button("Download it and recompile").clicked() {
                        action = Some(Action::DownloadCompiler(games));
                        self.compiler_offer = None;
                    }
                    if ui.button("Not now").clicked() {
                        self.compiler_offer = None;
                    }
                });
            });
        action
    }

    pub fn message(&mut self, ctx: &egui::Context) {
        let Some(text) = self.message.clone() else { return };
        // a line of recompiles that all fail makes it long, and it scrolls
        // to keep the OK that closes it on the screen
        let room = (ctx.content_rect().height() - 120.0).max(60.0);
        egui::Window::new("Zakuro")
            .id(egui::Id::new("message"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(room).show(ui, |ui| ui.label(text.as_str()));
                ui.horizontal(|ui| {
                    if let Some((_, report)) = self.report.as_ref().filter(|(about, _)| *about == text) {
                        if ui.button("Copy info for a report").clicked() {
                            ui.ctx().copy_text(report.clone());
                        }
                    }
                    if ui.button("OK").clicked() {
                        self.message = None;
                        self.report = None;
                    }
                });
            });
    }
}

fn rasterizer(ui: &mut egui::Ui, value: &mut bool) {
    ui.checkbox(value, "Draw the 3D on the GPU");
}

fn resolution(ui: &mut egui::Ui, value: &mut u32) {
    ui.add(egui::Slider::new(value, 1..=8).text("Resolution").suffix("x"))
        .on_hover_text("How many times the console's resolution the 3D is drawn at. Past what the window shows, the extra pixels smooth the edges, at a cost to the GPU that grows fast.");
}

fn texture_packs(ui: &mut egui::Ui, value: &mut bool) {
    ui.checkbox(value, "Texture packs")
        .on_hover_text("Draw a game's texture pack, made for Citra or Azahar, put in the textures folder of the game's mods folder.");
}

fn interpolation(ui: &mut egui::Ui, value: &mut bool) {
    ui.checkbox(value, "Frame interpolation")
        .on_hover_text("A game at 30 frames a second gets a picture drawn in between each two of its own, with everything halfway between where it was in both, so it moves at 60. The screens show a 60th of a second late. It takes the 3D drawn on the GPU and the Vulkan presenter.");
}

fn window_scale(ui: &mut egui::Ui, value: &mut u32) {
    ui.add(egui::Slider::new(value, 1..=6).text("Window scale"));
}

fn screens(ui: &mut egui::Ui, value: &mut Screens) {
    egui::ComboBox::from_label("Screens").selected_text(value.name()).show_ui(ui, |ui| {
        for layout in Screens::ALL {
            ui.selectable_value(value, layout, layout.name());
        }
    });
}

fn rotation(ui: &mut egui::Ui, value: &mut Orientation) {
    egui::ComboBox::from_label("Rotation").selected_text(value.name()).show_ui(ui, |ui| {
        for orientation in Orientation::ALL {
            ui.selectable_value(value, orientation, orientation.name());
        }
    });
}

fn filter(ui: &mut egui::Ui, value: &mut Filter) {
    egui::ComboBox::from_label("Filter")
        .selected_text(value.name())
        .show_ui(ui, |ui| {
            for filter in Filter::ALL {
                ui.selectable_value(value, filter, filter.name());
            }
        })
        .response
        .on_hover_text("How the screens' pixels look scaled up: smooth, sharp with soft edges, or plain squares.");
}

fn integer_scale(ui: &mut egui::Ui, value: &mut bool) {
    ui.checkbox(value, "Scale by whole numbers only").on_hover_text("Every pixel gets the same size, with a border around the screens.");
}

fn top_shape(ui: &mut egui::Ui, value: &mut TopShape) {
    egui::ComboBox::from_label("Top screen shape")
        .selected_text(value.name())
        .show_ui(ui, |ui| {
            for shape in TopShape::ALL {
                ui.selectable_value(value, shape, shape.name());
            }
        })
        .response
        .on_hover_text("Shows the top screen wider than the console's. Games still draw it at the console's shape, so only turn this on with a widescreen code for the game, which draws a wider scene to match.");
}

fn stereo_3d(ui: &mut egui::Ui, value: &mut Stereo3d) {
    egui::ComboBox::from_label("3D")
        .selected_text(value.name())
        .show_ui(ui, |ui| {
            for stereo in Stereo3d::ALL {
                ui.selectable_value(value, stereo, stereo.name());
            }
        })
        .response
        .on_hover_text("The top screen in 3D, for games that draw it: red and cyan for glasses of those colors, side by side for 3D TVs and headsets, alternate rows for passive 3D monitors. Off, games draw flat, as with the console's 3D slider down.");
}

fn depth_3d(ui: &mut egui::Ui, value: &mut u32) {
    ui.add(egui::Slider::new(value, 0..=100).text("Depth").suffix("%")).on_hover_text("The console's 3D slider, how deep games draw the 3D.");
}

fn language(ui: &mut egui::Ui, value: &mut Language) {
    egui::ComboBox::from_label("Language")
        .selected_text(value.name())
        .show_ui(ui, |ui| {
            for language in Language::ALL {
                ui.selectable_value(value, language, language.name());
            }
        })
        .response
        .on_hover_text("The console's system language, which titles pick their localised text and files by. It changes with the next game started, or Reset.");
}

/// what runs the game's code.
fn code(ui: &mut egui::Ui, recompiled: &mut bool) {
    ui.radio_value(recompiled, true, "Recompiled code when there is some");
    ui.radio_value(recompiled, false, "Interpreter only");
}

/// a line a GameOverlay keeps over the game, in the bottom right corner.
pub fn corner(ctx: &egui::Context, text: &str) {
    egui::Area::new(egui::Id::new("overlay corner"))
        .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-8.0, -8.0))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.label(text);
            });
        });
}

/// the games that aren't recompiling or waiting to, each once, the same
/// game being in the folder twice as a .3ds and a .cci say.
fn idle(games: impl Iterator<Item = Wanted>, jobs: &[Job], waiting: &[Wanted]) -> Vec<Wanted> {
    let busy = |id: u64| waiting.iter().any(|wanted| wanted.program_id == id) || jobs.iter().any(|job| job.program_id == id && !job.stage().finished());
    let mut idle: Vec<Wanted> = Vec::new();
    for game in games {
        if !busy(game.program_id) && !idle.iter().any(|wanted| wanted.program_id == game.program_id) {
            idle.push(game);
        }
    }
    idle
}

/// a recompile's progress bar with where it is.
fn progress(ui: &mut egui::Ui, job: &Job) {
    let stage = job.stage();
    let text = match &stage {
        Stage::Downloading(arrived) => format!("downloading the compiler, {:.0}%", arrived * 100.0),
        Stage::Generating => "finding the code".to_owned(),
        Stage::Compiling { done, total } => format!("compiling {done} of {total}"),
        Stage::Installing => "installing".to_owned(),
        Stage::Done => "done".to_owned(),
        Stage::Failed(error) => format!("failed, {error}"),
    };
    let minutes = job.elapsed().as_secs() / 60;
    let seconds = job.elapsed().as_secs() % 60;
    ui.add(egui::ProgressBar::new(stage.fraction()).desired_width(220.0).text(format!("{text}, {minutes}:{seconds:02}")));
}

/// a key as a person would call it.
pub fn key_name(key: KeyCode) -> String {
    let name = format!("{key:?}");
    for prefix in ["Key", "Digit", "Arrow"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return rest.to_owned();
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_background_fills_the_area_without_stretching() {
        // a square picture in a wide area loses its top and bottom
        let wide = cover(Vec2::new(200.0, 100.0), [100, 100]);
        assert_eq!((wide.min.x, wide.max.x), (0.0, 1.0));
        assert_eq!((wide.min.y, wide.max.y), (0.25, 0.75));
        // and in a tall one its sides
        let tall = cover(Vec2::new(100.0, 200.0), [100, 100]);
        assert_eq!((tall.min.x, tall.max.x), (0.25, 0.75));
        assert_eq!((tall.min.y, tall.max.y), (0.0, 1.0));
    }

    #[test]
    fn big_pictures_are_scaled_down_keeping_their_shape() {
        let path = std::env::temp_dir().join(format!("zakuro-background-{}.png", std::process::id()));
        image::RgbaImage::from_pixel(BACKGROUND_SIDE * 2, BACKGROUND_SIDE, image::Rgba([10, 20, 30, 255])).save(&path).unwrap();
        let decoded = decode(&path);
        std::fs::remove_file(&path).ok();
        let decoded = decoded.unwrap();
        assert_eq!(decoded.size, [BACKGROUND_SIDE as usize, BACKGROUND_SIDE as usize / 2]);
        assert_eq!(decoded.pixels[0].to_array(), [10, 20, 30, 255]);
        assert!(decode(Path::new("/nowhere/at/all.png")).is_err());
    }

    /// the settings stay inside a short screen, scrolling in it instead of
    /// running past its bottom.
    #[test]
    fn the_settings_fit_a_short_screen() {
        let ctx = egui::Context::default();
        let mut menus = Menus { settings_open: true, ..Menus::default() };
        let mut settings = Settings::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 360.0));
        for _ in 0..3 {
            let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            ctx.run_ui(input, |ui| {
                menus.settings(ui.ctx(), &mut settings, false);
            })
            .drop_without_applying_deltas();
        }
        let window = ctx.memory(|memory| memory.area_rect(egui::Id::new("settings"))).unwrap();
        assert!(screen.contains_rect(window), "{window:?} in {screen:?}");
    }

    /// runs a frame of a panel with one row of a game's settings, clicking
    /// where the row starts when asked to.
    fn frame_of_a_row(ctx: &egui::Context, click: bool, mut row: impl FnMut(&mut egui::Ui)) {
        let at = egui::pos2(16.0, 16.0);
        let mut events = vec![egui::Event::PointerMoved(at)];
        if click {
            let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            events.extend([press(true), press(false)]);
        }
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(400.0, 200.0));
        let input = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
        let panel = egui::Frame::NONE.inner_margin(8.0);
        ctx.run_ui(input, |ui| drop(panel.show(ui, |ui| row(ui)))).drop_without_applying_deltas();
    }

    /// rendering a game setting row when following global and when overridden
    #[test]
    fn game_setting_row_renders() {
        let ctx = egui::Context::default();
        frame_of_a_row(&ctx, false, |ui| {
            let _ = game_setting_row(ui, false, false, "Resolution", false, "", "1x");
        });
        frame_of_a_row(&ctx, false, |ui| {
            let _ = game_setting_row(ui, true, false, "Resolution", true, "3x", "1x");
        });
    }

    /// a game's settings go in the file only while it has some of its own,
    /// and fit a short screen as the general ones do.
    #[test]
    fn a_games_settings_fit_a_short_screen() {
        let ctx = egui::Context::default();
        let mut menus = Menus { game_settings: Some((0x0004_0000_0012_3400, "A game".to_owned())), ..Menus::default() };
        let mut settings = Settings::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 360.0));
        for _ in 0..3 {
            let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            ctx.run_ui(input, |ui| {
                menus.game_settings(ui.ctx(), &mut settings);
            })
            .drop_without_applying_deltas();
        }
        let window = ctx.memory(|memory| memory.area_rect(egui::Id::new("game settings"))).unwrap();
        assert!(screen.contains_rect(window), "{window:?} in {screen:?}");
        assert!(settings.per_game.is_empty(), "opening the window gives the game nothing");
    }

    /// however long a message gets, its OK stays on the screen.
    #[test]
    fn a_long_message_can_still_be_closed() {
        let ctx = egui::Context::default();
        let mut menus = Menus { message: Some("Recompiling a game failed, it did.\n\n".repeat(200)), ..Menus::default() };
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 480.0));
        for _ in 0..3 {
            let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
            ctx.run_ui(input, |ui| menus.message(ui.ctx())).drop_without_applying_deltas();
        }
        let window = ctx.memory(|memory| memory.area_rect(egui::Id::new("message"))).unwrap();
        assert!(screen.contains_rect(window), "{window:?} in {screen:?}");
    }

    /// a game in line, or having its turn, isn't offered again.
    #[test]
    fn games_already_in_line_are_left_out() {
        let wanted = |program_id, path: &str| Wanted { program_id, path: path.into() };
        let games = || [wanted(1, "a"), wanted(2, "b"), wanted(3, "c")].into_iter();
        let ids = |games: Vec<Wanted>| games.into_iter().map(|game| game.program_id).collect::<Vec<_>>();
        assert_eq!(ids(idle(games(), &[], &[])), [1, 2, 3]);
        let twice = idle([wanted(1, "a.3ds"), wanted(2, "b"), wanted(1, "a.cci")].into_iter(), &[], &[]);
        assert_eq!(twice, [wanted(1, "a.3ds"), wanted(2, "b")], "the same game twice in the folder");
        assert_eq!(ids(idle(games(), &[], &[wanted(2, "b")])), [1, 3]);
        let job = Job::at_stage(1, Stage::Compiling { done: 1, total: 2 });
        assert_eq!(ids(idle(games(), std::slice::from_ref(&job), &[wanted(2, "b")])), [3]);
        let job = Job::at_stage(1, Stage::Failed("no".to_owned()));
        assert_eq!(ids(idle(games(), std::slice::from_ref(&job), &[])), [1, 2, 3]);
    }

    #[test]
    fn keys_have_short_names() {
        assert_eq!(key_name(KeyCode::KeyX), "X");
        assert_eq!(key_name(KeyCode::ArrowUp), "Up");
        assert_eq!(key_name(KeyCode::Digit1), "1");
        assert_eq!(key_name(KeyCode::Enter), "Enter");
    }
}
