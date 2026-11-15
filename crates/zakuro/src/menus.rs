//! the interface, drawn with egui: the Home screen with the games, the
//! menu over a running game, the system settings, each game's options, and
//! the windows that come up over them.
//!
//! every list in it is made of the same rows, built from the settings each
//! frame, which the mouse, a finger, the keyboard and a controller all move
//! through the same way: hovering or pressing a direction selects a row,
//! clicking it or pressing A does what it does, and left and right change a
//! value. a game's options are the system settings' own rows, showing what
//! the game plays with; changing one there keeps it for that game alone and
//! marks it with a dot, and X or the row's reset puts it back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use egui::{Align2, Color32, CornerRadius, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use winit::keyboard::KeyCode;
use zakuro_core::services::keyboard::Request;

use crate::gamepad::button_name;
use crate::icons::Icon;
use crate::library::{Game, Library, ICON_SIZE};
use crate::nav::{InputMode, Nav};
use crate::recompile::{Job, Stage, Wanted};
use crate::settings::{
    game_key, CameraSource, Filter, FpsLimit, Keys, Language, MicrophoneSource, Orientation, PadButtons, Renderer, Screens, Settings, Stereo3d, Theme, TopShape,
};
use crate::store::{self, Found, InstalledPack, PackPlace};

/// what the player asked for, which the app does.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Play(PathBuf),
    /// recompile these games of the library, one after the other.
    Recompile(Vec<Wanted>),
    Mods(u64),
    /// stop recompiling the game with this id, or waiting to.
    CancelRecompile(u64),
    /// leave only the game recompiling now, none of those waiting.
    CancelWaiting,
    ChooseBackground,
    /// pick the picture the cameras see.
    ChooseCameraPicture,
    /// pick games, texture packs and cheats to add, for the game with this
    /// id when one is given.
    AddContent(Option<u64>),
    /// pick a folder holding a texture pack, for the game with this id.
    AddTextures(u64),
    /// add what the player chose of what was found.
    Install(store::Choice),
    CancelInstall,
    /// take a texture pack out of a game.
    RemovePack(InstalledPack),
    /// takes the game of this id out of the library.
    RemoveGame(u64),
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

/// whose settings a row changes: the system's, or one game's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    System,
    Game(u64),
}

/// a row of a list, what it is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    // graphics
    Renderer,
    Theme,
    Rasterizer,
    Resolution,
    Filter,
    TexturePacks,
    SmoothTextures,
    Interpolation,
    FpsLimit,
    // screens
    Layout,
    Orientation,
    IntegerScale,
    TopShape,
    Fullscreen,
    ShowFps,
    // 3D
    Stereo,
    Depth,
    SwapEyes,
    // controls
    Pad(usize),
    PadDefaults,
    Key(usize),
    KeyDefaults,
    // audio
    Volume,
    Mute,
    Microphone,
    // library
    AddContent,
    Background,
    BackgroundOpacity,
    RecompileOutdated,
    // system
    Language,
    Recompiled,
    Camera,
    CameraPicture,
    CopyReport,
    // a game's add-ons
    Pack(usize),
    AddPack,
    Cheats,
    OpenMods,
    Recompile,
    UseSystem,
    RemoveGame,
    /// answers a recompile Zakuro asks about.
    Ask,
    // the menu over a game
    Resume,
    Slot,
    SaveState,
    LoadState,
    Options,
    SystemSettings,
    Restart,
    Home,
    Quit,
    // a choice in a list that came up
    Choice(usize),
    // what was found to add
    ImportFound(usize),
    ImportFor(usize, u64),
    ImportPlace,
    ImportAdd,
    ImportCancel,
    // the cheats window
    Cheat(usize),
    CheatAdd,
    CheatsReload,
    CheatsFolder,
}

/// what is done to a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// A, Enter or a click.
    Activate,
    /// left or right.
    Step(i32),
    /// X, back to the system's say.
    Reset,
}

/// what a row shows on its right.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    None,
    /// a switch.
    On(bool),
    /// a value picked from a list, or an action's state.
    Text(String),
    /// a value left and right change, shown with arrows when selected.
    Range(String),
    /// an action that opens something.
    More,
    /// the choice in force, in a list to pick from.
    Check,
}

#[derive(Debug, Clone, PartialEq)]
struct Row {
    item: Item,
    label: String,
    value: Value,
    /// a line under the label.
    note: Option<String>,
    enabled: bool,
    /// a game's own value, not the system's.
    changed: bool,
    /// the action is one that loses something.
    danger: bool,
    /// what the setting does, shown at the foot of the list while it is
    /// selected.
    help: Option<&'static str>,
}

impl Row {
    fn new(item: Item, label: impl Into<String>, value: Value) -> Row {
        Row { item, label: label.into(), value, note: None, enabled: true, changed: false, danger: false, help: None }
    }

    fn note(mut self, note: impl Into<String>) -> Row {
        self.note = Some(note.into());
        self
    }

    fn enabled(mut self, enabled: bool) -> Row {
        self.enabled = enabled;
        self
    }

    fn danger(mut self) -> Row {
        self.danger = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Entry {
    Header(String),
    Row(Row),
}

/// the rows of a list, with the headers between them left out.
fn rows(entries: &[Entry]) -> impl Iterator<Item = &Row> {
    entries.iter().filter_map(|entry| match entry {
        Entry::Row(row) => Some(row),
        Entry::Header(_) => None,
    })
}

/// a section of the system settings, or of a game's options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Graphics,
    Screens,
    Stereo,
    Controls,
    Audio,
    Library,
    System,
    AddOns,
}

impl Section {
    pub const SYSTEM: [Section; 7] = [Section::Graphics, Section::Screens, Section::Stereo, Section::Controls, Section::Audio, Section::Library, Section::System];
    pub const GAME: [Section; 5] = [Section::Graphics, Section::Screens, Section::Stereo, Section::System, Section::AddOns];

    pub fn name(self) -> &'static str {
        match self {
            Section::Graphics => "Graphics",
            Section::Screens => "Display",
            Section::Stereo => "3D",
            Section::Controls => "Controls",
            Section::Audio => "Audio",
            Section::Library => "Library",
            Section::System => "System",
            Section::AddOns => "Add-ons",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Section::Graphics => Icon::Monitor,
            Section::Screens => Icon::Screens,
            Section::Stereo => Icon::Cube,
            Section::Controls => Icon::Gamepad,
            Section::Audio => Icon::Speaker,
            Section::Library => Icon::Folder,
            Section::System => Icon::Chip,
            Section::AddOns => Icon::AddOns,
        }
    }
}

/// a list that came up to pick from.
#[derive(Clone, Debug)]
pub struct SelectPopup {
    /// the setting picked for, and whose.
    pub item: Item,
    pub scope: Scope,
    pub title: String,
    pub options: Vec<String>,
    pub selected: usize,
    /// the choice in force, ticked, when picking a setting.
    pub current: Option<usize>,
}

/// a recompile Zakuro asks the player about.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    /// before playing a game whose code isn't recompiled, or is outdated.
    Play { game: Wanted, name: String },
    /// games just added, or given an update or DLC.
    Added(Vec<(Wanted, String)>),
    /// recompiling picked in a game's options.
    Explain { game: Wanted, name: String },
}

/// what recompiling does, for the player deciding on it.
const RECOMPILE_EXPLAINED: &str = "Recompiling turns the game's code into native code for this computer, so it runs faster and smoother. It takes about ten minutes and the whole processor, so no game can be started until it finishes, and it is done once per game.";

impl Ask {
    /// the question and its answers, the first one recompiling. another
    /// recompile under way or waiting goes first.
    pub fn popup(&self, waiting_for: Option<&str>) -> (String, Vec<String>) {
        let queued = waiting_for.map_or(String::new(), |other| format!(" It starts once {other} finishes."));
        let recompile = if waiting_for.is_some() { "Recompile after that" } else { "Recompile now" };
        match self {
            Ask::Play { name, .. } => (
                format!("{name} isn't recompiled, or its code changed since it was. {RECOMPILE_EXPLAINED}{queued}"),
                vec![recompile.to_owned(), "Play without".to_owned(), "Don't ask again for this game".to_owned()],
            ),
            Ask::Added(games) => {
                let names = match games.as_slice() {
                    [(_, name)] => name.clone(),
                    games => format!("the {} games", games.len()),
                };
                (format!("Recompile {names}? New games, updates and DLC need it to run their best. {RECOMPILE_EXPLAINED}{queued}"), vec![recompile.to_owned(), "Not now".to_owned()])
            }
            Ask::Explain { name, .. } => (format!("Recompile {name}? {RECOMPILE_EXPLAINED}{queued}"), vec![recompile.to_owned(), "Not now".to_owned()]),
        }
    }

    /// what the answer at index does.
    fn answer(self, index: usize, settings: &mut Settings) -> Vec<Action> {
        match (self, index) {
            (Ask::Play { game, .. } | Ask::Explain { game, .. }, 0) => vec![Action::Recompile(vec![game])],
            (Ask::Added(games), 0) => vec![Action::Recompile(games.into_iter().map(|(game, _)| game).collect())],
            (Ask::Play { game, .. }, 1) => vec![Action::Play(game.path)],
            (Ask::Play { game, .. }, 2) => {
                let key = game_key(game.program_id);
                if !settings.dont_ask_recompile.contains(&key) {
                    settings.dont_ask_recompile.push(key);
                }
                vec![Action::Play(game.path)]
            }
            _ => Vec::new(),
        }
    }
}

/// what was found to add, waiting for the player to say what goes where.
#[derive(Debug, Clone)]
pub struct ImportView {
    /// each thing found, whether it is added, and the games it goes to.
    pub found: Vec<(Found, bool, Vec<u64>)>,
    pub place: PackPlace,
}

/// a running game, as the interface shows it.
pub struct RunningView<'a> {
    pub name: &'a str,
    pub program_id: Option<u64>,
    pub recompiled: bool,
    /// when the slot chosen was saved to.
    pub saved: Option<&'a str>,
    pub fps: Option<f32>,
    pub fast: bool,
    /// the settings the game started with, for those it reads only then.
    pub started: Option<&'a Settings>,
}

/// a running game's cheat.
pub struct CheatView {
    pub name: String,
    pub enabled: bool,
    pub notes: String,
    /// one of Zakuro's own enhancements.
    pub builtin: bool,
}

/// what the interface shows, besides the settings.
pub struct View<'a> {
    pub library: &'a Library,
    pub jobs: &'a [Job],
    pub waiting: &'a [Wanted],
    pub running: Option<RunningView<'a>>,
    pub cheats: &'a [CheatView],
    pub install: Option<store::Progress>,
    /// what was dropped or picked is being looked at.
    pub identifying: bool,
    pub room: Option<&'a RoomView>,
    pub keyboard: Option<&'a Request>,
    /// a line a GameOverlay keeps over the game.
    pub corner: Option<&'a str>,
    pub data_dir: Option<&'a Path>,
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

/// where the selection is on the Home screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HomeFocus {
    #[default]
    Games,
    Dock,
}

/// the round buttons under the games.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dock {
    Add,
    Multiplayer,
    Settings,
    Quit,
}

const DOCK: [Dock; 4] = [Dock::Add, Dock::Multiplayer, Dock::Settings, Dock::Quit];

impl Dock {
    fn name(self) -> &'static str {
        match self {
            Dock::Add => "Add games",
            Dock::Multiplayer => "Multiplayer",
            Dock::Settings => "System Settings",
            Dock::Quit => "Quit Zakuro",
        }
    }
}

/// the page of settings open: the system's, or a game's options.
#[derive(Debug, Clone, Default)]
struct Page {
    section: usize,
    row: usize,
    /// the selection is in the list rather than the sections.
    in_list: bool,
    scroll: bool,
}

#[derive(Default)]
pub struct Menus {
    /// what was used last, mouse or buttons, which decides what shows.
    pub mode: InputMode,
    /// the buttons were a controller's rather than the keyboard's, for the
    /// hints.
    pub pad_hints: bool,
    /// presses waiting for the next frame.
    queue: Vec<Nav>,
    /// rows clicked while drawing, done once the frame is drawn, and the
    /// like for Home's tiles and buttons, a cheat typed and an install
    /// stopped.
    clicked: Vec<(Item, Op)>,
    clicked_tile: Option<usize>,
    clicked_dock: Option<Dock>,
    /// when the running game first showed, Home fading out over it.
    game_shown_at: Option<f64>,
    pending_cheat: Option<(String, String)>,
    pending_cancel_install: bool,

    /// the menu over a running game.
    pub menu_open: bool,
    pause_row: usize,
    /// the slot states are saved to and loaded from, 1 to 9.
    pub slot: u8,

    /// the game selected on the Home screen, and the dock button.
    pub selected_game: usize,
    pub home_focus: HomeFocus,
    dock: usize,
    /// how far the games are scrolled, in tiles.
    home_scroll: f32,

    pub settings_open: bool,
    /// the game whose options are open, its id and its name.
    pub game_settings: Option<(u64, String)>,
    page: Page,
    /// the texture packs of the game whose options are open.
    packs: Vec<InstalledPack>,

    /// the binding waiting for a key, by its place in Keys::all_mut, or a
    /// button, by its place in PadButtons::all_mut.
    pub rebinding: Option<usize>,
    pub rebinding_pad: Option<usize>,

    pub cheats_open: bool,
    cheats_row: usize,
    /// the cheat being written, shown while adding one.
    cheat_form: bool,
    cheat_name: String,
    cheat_code: String,

    pub multiplayer_open: bool,
    pub room_password: String,
    /// the icons of the games offered for Download Play.
    dlp_icons: HashMap<u64, egui::TextureHandle>,

    /// a list to pick from.
    pub select_popup: Option<SelectPopup>,
    /// the recompile the select popup asks about.
    pub ask: Option<Ask>,
    /// what was found to add.
    pub import: Option<ImportView>,
    import_row: usize,

    /// something to tell the user, until they close it.
    pub message: Option<String>,
    /// something to tell the user for a moment, and since when.
    pub toast: Option<(String, std::time::Instant)>,
    /// a report on the game, to copy from the message it goes with, and
    /// from no other.
    pub report: Option<(String, String)>,
    /// what is being typed into the game's keyboard, and what it asked for.
    typing: Option<(Request, String)>,

    icons: HashMap<PathBuf, egui::TextureHandle>,
    background: Background,
}

/// the colors: slate greys with one violet accent, in a dark and a light
/// set. text is 4.5:1 or more against what it sits on in both.
pub mod color {
    use std::sync::atomic::{AtomicBool, Ordering};

    use egui::Color32;

    struct Palette {
        bg: Color32,
        surface: Color32,
        raised: Color32,
        hover: Color32,
        line: Color32,
        text: Color32,
        muted: Color32,
        faint: Color32,
        accent: Color32,
        danger: Color32,
        warn: Color32,
        on: Color32,
        /// a switch turned off, which the white knob must stand out on.
        track: Color32,
        /// a window's body.
        panel: Color32,
        scrim: u8,
    }

    const fn rgb(hex: u32) -> Color32 {
        Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    const DARK: Palette = Palette {
        bg: rgb(0x0B1120),
        surface: rgb(0x151C2C),
        raised: rgb(0x1F2738),
        hover: rgb(0x283245),
        line: rgb(0x2A3446),
        text: rgb(0xF1F5F9),
        muted: rgb(0x94A3B8),
        faint: rgb(0x718098),
        accent: rgb(0xA78BFA),
        danger: rgb(0xF87171),
        warn: rgb(0xFBBF24),
        on: rgb(0x8B5CF6),
        track: rgb(0x334155),
        panel: rgb(0x0B1120),
        scrim: 170,
    };

    const LIGHT: Palette = Palette {
        bg: rgb(0xEEF2F7),
        surface: rgb(0xFFFFFF),
        raised: rgb(0xE8EDF4),
        hover: rgb(0xDDE4EE),
        line: rgb(0xD5DDE8),
        text: rgb(0x0F172A),
        muted: rgb(0x475569),
        faint: rgb(0x64748B),
        accent: rgb(0x7C3AED),
        danger: rgb(0xDC2626),
        warn: rgb(0xB45309),
        on: rgb(0x7C3AED),
        track: rgb(0xB8C3D3),
        panel: rgb(0xF8FAFC),
        scrim: 90,
    };

    static IS_LIGHT: AtomicBool = AtomicBool::new(false);

    /// picks the set the interface is drawn in from now on.
    pub fn set_light(light: bool) {
        IS_LIGHT.store(light, Ordering::Relaxed);
    }

    pub fn is_light() -> bool {
        IS_LIGHT.load(Ordering::Relaxed)
    }

    fn palette() -> &'static Palette {
        if is_light() { &LIGHT } else { &DARK }
    }

    pub fn bg() -> Color32 { palette().bg }
    pub fn surface() -> Color32 { palette().surface }
    pub fn raised() -> Color32 { palette().raised }
    pub fn hover() -> Color32 { palette().hover }
    pub fn line() -> Color32 { palette().line }
    pub fn text() -> Color32 { palette().text }
    pub fn muted() -> Color32 { palette().muted }
    pub fn faint() -> Color32 { palette().faint }
    pub fn accent() -> Color32 { palette().accent }
    pub fn danger() -> Color32 { palette().danger }
    pub fn warn() -> Color32 { palette().warn }
    pub fn on() -> Color32 { palette().on }
    pub fn track() -> Color32 { palette().track }
    pub fn panel() -> Color32 { palette().panel }

    /// the dimming laid over what is behind a window.
    pub fn scrim() -> Color32 {
        Color32::from_black_alpha(palette().scrim)
    }

    /// egui's own widgets in the same colors.
    pub fn visuals(light: bool) -> egui::Visuals {
        let p = if light { &LIGHT } else { &DARK };
        let mut visuals = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
        visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
        visuals.panel_fill = p.bg;
        visuals.window_fill = p.surface;
        visuals.window_stroke = egui::Stroke::new(1.0, p.line);
        visuals.extreme_bg_color = p.raised;
        visuals.faint_bg_color = p.raised;
        visuals.override_text_color = Some(p.text);
        visuals.hyperlink_color = p.accent;
        visuals.window_corner_radius = egui::CornerRadius::same(12);
        visuals.menu_corner_radius = egui::CornerRadius::same(8);
        let widgets = [
            (&mut visuals.widgets.noninteractive, p.surface, p.line, p.muted),
            (&mut visuals.widgets.inactive, p.raised, p.line, p.text),
            (&mut visuals.widgets.hovered, p.hover, p.accent, p.text),
            (&mut visuals.widgets.active, p.hover, p.accent, p.text),
            (&mut visuals.widgets.open, p.raised, p.line, p.text),
        ];
        for (widget, fill, stroke, text) in widgets {
            widget.corner_radius = egui::CornerRadius::same(8);
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.bg_stroke = egui::Stroke::new(1.0, stroke);
            widget.fg_stroke = egui::Stroke::new(1.0, text);
        }
        visuals.selection.bg_fill = p.accent.gamma_multiply(0.35);
        visuals.selection.stroke = egui::Stroke::new(1.5, p.accent);
        visuals
    }
}

pub const RESOLUTION_SCALES: [u32; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// a resolution scale, short, with the height of the top screen it draws.
pub fn resolution_label(scale: u32) -> &'static str {
    match scale {
        1 => "1x · Native",
        2 => "2x · 480p",
        3 => "3x · 720p",
        4 => "4x · 960p",
        5 => "5x · 1200p",
        6 => "6x · 1440p",
        7 => "7x · 1680p",
        8 => "8x · 4K",
        _ => "Custom",
    }
}

/// the size of the top screen a resolution scale draws, for under it in
/// the list it is picked from.
pub fn resolution_size(scale: u32) -> String {
    format!("{} × {}", 400 * scale, 240 * scale)
}

/// a line for each of the choices of a setting, what it does, when they
/// need one.
fn choice_notes(item: Item) -> Vec<String> {
    match item {
        Item::Filter => Filter::ALL.iter().map(|filter| filter.describe().to_owned()).collect(),
        Item::Resolution => RESOLUTION_SCALES.iter().map(|&scale| resolution_size(scale)).collect(),
        Item::Rasterizer => vec!["Fast, and needed for the settings above".into(), "Slow, but draws exactly as the console does".into()],
        Item::Renderer => vec!["Fastest, and needed for frame interpolation".into(), "Try it if a game shows glitches".into()],
        _ => Vec::new(),
    }
}

/// the choices of how the 3D is drawn, the GPU's first.
const RASTERIZERS: [&str; 2] = ["GPU", "CPU"];

fn names<T: Copy + PartialEq>(all: &[T], current: T, name: impl Fn(T) -> &'static str) -> (Vec<String>, usize) {
    (all.iter().map(|&item| name(item).to_owned()).collect(), all.iter().position(|&item| item == current).unwrap_or(0))
}

/// sets a setting the system's, or the game's own when it differs from the
/// system's, with nothing of its own when it doesn't.
macro_rules! put {
    ($settings:expr, $scope:expr, $field:ident, $value:expr) => {{
        let value = $value;
        match $scope {
            Scope::System => $settings.$field = value,
            Scope::Game(id) => {
                let system = $settings.$field;
                $settings.game_mut(id).$field = (value != system).then_some(value);
                $settings.prune();
            }
        }
    }};
}

/// whether the game has a setting of its own, and taking it away.
macro_rules! own_fields {
    ($own:expr, $item:expr, |$field:ident| $body:expr) => {
        match $item {
            Item::Rasterizer => { let $field = &mut $own.hardware_rasterizer; $body }
            Item::Resolution => { let $field = &mut $own.resolution; $body }
            Item::Filter => { let $field = &mut $own.filter; $body }
            Item::TexturePacks => { let $field = &mut $own.texture_packs; $body }
            Item::SmoothTextures => { let $field = &mut $own.smooth_textures; $body }
            Item::Interpolation => { let $field = &mut $own.frame_interpolation; $body }
            Item::FpsLimit => { let $field = &mut $own.fps_limit; $body }
            Item::Layout => { let $field = &mut $own.layout; $body }
            Item::Orientation => { let $field = &mut $own.orientation; $body }
            Item::IntegerScale => { let $field = &mut $own.integer_scale; $body }
            Item::TopShape => { let $field = &mut $own.top_shape; $body }
            Item::Stereo => { let $field = &mut $own.stereo; $body }
            Item::Depth => { let $field = &mut $own.depth_3d; $body }
            Item::Language => { let $field = &mut $own.language; $body }
            Item::Recompiled => { let $field = &mut $own.recompiled; $body }
            _ => false,
        }
    };
}

/// whether a game can have a setting of its own.
#[cfg(test)]
fn game_can_own(item: Item) -> bool {
    let mut own = crate::settings::GameSettings::default();
    own_fields!(own, item, |field| {
        let _ = field;
        true
    })
}

/// whether the game has the setting of its own.
fn owned(settings: &Settings, scope: Scope, item: Item) -> bool {
    let Scope::Game(id) = scope else { return false };
    let Some(own) = settings.per_game.get(&game_key(id)) else { return false };
    let mut own = own.clone();
    own_fields!(own, item, |field| field.is_some())
}

/// the choices a setting picks from, and the one in force.
fn choices(item: Item, settings: &Settings) -> Option<(Vec<String>, usize)> {
    Some(match item {
        Item::Renderer => names(&Renderer::ALL, settings.renderer, Renderer::name),
        Item::Theme => names(&Theme::ALL, settings.theme, Theme::name),
        Item::Rasterizer => (RASTERIZERS.iter().map(|&name| name.to_owned()).collect(), usize::from(!settings.hardware_rasterizer)),
        Item::Resolution => (RESOLUTION_SCALES.iter().map(|&scale| resolution_label(scale).to_owned()).collect(), settings.resolution.clamp(1, 8) as usize - 1),
        Item::Filter => names(&Filter::ALL, settings.filter, Filter::name),
        Item::FpsLimit => names(&FpsLimit::ALL, settings.fps_limit, FpsLimit::badge_name),
        Item::Layout => names(&Screens::ALL, settings.layout, Screens::name),
        Item::Orientation => names(&Orientation::ALL, settings.orientation, Orientation::name),
        Item::TopShape => names(&TopShape::ALL, settings.top_shape, TopShape::name),
        Item::Stereo => names(&Stereo3d::ALL, settings.stereo, Stereo3d::name),
        Item::Microphone => names(&MicrophoneSource::ALL, settings.microphone, MicrophoneSource::name),
        Item::Language => names(&Language::ALL, settings.language, Language::name),
        Item::Camera => names(&CameraSource::ALL, settings.camera, CameraSource::name),
        _ => return None,
    })
}

/// picks a setting's choice by its place in the list.
fn choose(item: Item, index: usize, settings: &mut Settings, scope: Scope) {
    match item {
        Item::Renderer => settings.renderer = Renderer::ALL[index.min(Renderer::ALL.len() - 1)],
        Item::Theme => settings.theme = Theme::ALL[index.min(Theme::ALL.len() - 1)],
        Item::Rasterizer => put!(settings, scope, hardware_rasterizer, index == 0),
        Item::Resolution => put!(settings, scope, resolution, index.min(7) as u32 + 1),
        Item::Filter => put!(settings, scope, filter, Filter::ALL[index.min(Filter::ALL.len() - 1)]),
        Item::FpsLimit => put!(settings, scope, fps_limit, FpsLimit::ALL[index.min(FpsLimit::ALL.len() - 1)]),
        Item::Layout => put!(settings, scope, layout, Screens::ALL[index.min(Screens::ALL.len() - 1)]),
        Item::Orientation => put!(settings, scope, orientation, Orientation::ALL[index.min(Orientation::ALL.len() - 1)]),
        Item::TopShape => put!(settings, scope, top_shape, TopShape::ALL[index.min(TopShape::ALL.len() - 1)]),
        Item::Stereo => put!(settings, scope, stereo, Stereo3d::ALL[index.min(Stereo3d::ALL.len() - 1)]),
        Item::Language => put!(settings, scope, language, Language::ALL[index.min(Language::ALL.len() - 1)]),
        Item::Microphone => settings.microphone = MicrophoneSource::ALL[index.min(MicrophoneSource::ALL.len() - 1)],
        Item::Camera => settings.camera = CameraSource::ALL[index.min(CameraSource::ALL.len() - 1)],
        _ => {}
    }
}

/// flips a switch.
fn toggle(item: Item, settings: &mut Settings, scope: Scope) {
    let shown = settings.for_game(match scope {
        Scope::Game(id) => Some(id),
        Scope::System => None,
    });
    match item {
        Item::TexturePacks => put!(settings, scope, texture_packs, !shown.texture_packs),
        Item::SmoothTextures => put!(settings, scope, smooth_textures, !shown.smooth_textures),
        Item::Interpolation => put!(settings, scope, frame_interpolation, !shown.frame_interpolation),
        Item::IntegerScale => put!(settings, scope, integer_scale, !shown.integer_scale),
        Item::Recompiled => put!(settings, scope, recompiled, !shown.recompiled),
        Item::SwapEyes => settings.swap_eyes = !settings.swap_eyes,
        Item::ShowFps => settings.show_fps = !settings.show_fps,
        Item::Mute => settings.mute = !settings.mute,
        _ => {}
    }
}

/// moves a value along, a choice to the next or a number up or down.
fn step(item: Item, delta: i32, settings: &mut Settings, scope: Scope) {
    let shown = settings.for_game(match scope {
        Scope::Game(id) => Some(id),
        Scope::System => None,
    });
    if let Some((options, current)) = choices(item, &shown) {
        let count = options.len() as i32;
        // resolutions stop at the ends, lists go round
        let next = if item == Item::Resolution { (current as i32 + delta).clamp(0, count - 1) } else { (current as i32 + delta).rem_euclid(count) };
        choose(item, next as usize, settings, scope);
        return;
    }
    match item {
        Item::Depth => put!(settings, scope, depth_3d, (shown.depth_3d as i32 + delta * 5).clamp(0, 100) as u32),
        Item::Volume => settings.volume = ((settings.volume * 20.0).round() + delta as f32).clamp(0.0, 20.0) / 20.0,
        Item::BackgroundOpacity => settings.background_opacity = ((settings.background_opacity * 20.0).round() + delta as f32).clamp(0.0, 20.0) / 20.0,
        _ if matches!(value_kind(item), Kind::Switch) => toggle(item, settings, scope),
        _ => {}
    }
}

enum Kind {
    Choice,
    Switch,
    Range,
    Other,
}

fn value_kind(item: Item) -> Kind {
    match item {
        Item::Renderer
        | Item::Theme
        | Item::Rasterizer
        | Item::Resolution
        | Item::Filter
        | Item::FpsLimit
        | Item::Layout
        | Item::Orientation
        | Item::TopShape
        | Item::Stereo
        | Item::Microphone
        | Item::Language
        | Item::Camera => Kind::Choice,
        Item::TexturePacks | Item::SmoothTextures | Item::Interpolation | Item::IntegerScale | Item::Recompiled | Item::SwapEyes | Item::ShowFps | Item::Mute => {
            Kind::Switch
        }
        Item::Depth | Item::Volume | Item::BackgroundOpacity | Item::Slot => Kind::Range,
        _ => Kind::Other,
    }
}

/// a setting's row, showing what is in force for scope.
fn setting(item: Item, settings: &Settings, scope: Scope) -> Row {
    let shown = settings.for_game(match scope {
        Scope::Game(id) => Some(id),
        Scope::System => None,
    });
    let hardware = shown.hardware_rasterizer;
    let stereo = shown.stereo != Stereo3d::Off;
    let file = |path: &Option<PathBuf>| path.as_ref().and_then(|path| path.file_name()).map_or("None".into(), |name| name.to_string_lossy().into_owned());
    let (label, value, help): (&str, Value, &str) = match item {
        Item::Theme => ("Theme", Value::Text(settings.theme.name().into()), "Light or dark. Same as system follows Windows."),
        Item::Renderer => ("Graphics API", Value::Text(shown.renderer.name().into()), "Vulkan is the fastest. Switch to OpenGL only if a game shows glitches."),
        Item::Rasterizer => (
            "3D rendering",
            Value::Text(RASTERIZERS[usize::from(!hardware)].into()),
            "The GPU is fast and makes the resolution, textures and frame interpolation possible. The CPU is slow but exact.",
        ),
        Item::Resolution => ("Resolution", Value::Text(resolution_label(shown.resolution).into()), "How finely the 3D is drawn. Higher looks cleaner and needs a faster GPU."),
        Item::Filter => ("Display filter", Value::Text(shown.filter.name().into()), "How the picture is fitted to your screen."),
        Item::SmoothTextures => ("Upscale textures", Value::On(shown.smooth_textures), "Redraws the game's low-resolution textures with clean, smooth edges."),
        Item::TexturePacks => ("Custom textures", Value::On(shown.texture_packs), "Swaps in textures from packs added to a game, often HD ones. Add them in its Options, under Add-ons."),
        Item::Interpolation => ("Frame interpolation", Value::On(shown.frame_interpolation), "Makes 30 FPS games as smooth as 60, with a frame more of delay."),
        Item::FpsLimit => ("Frame rate limit", Value::Text(shown.fps_limit.badge_name().into()), "How often the screen updates. Games always run at their normal speed."),
        Item::Layout => ("Screen layout", Value::Text(shown.layout.name().into()), "How the two screens are arranged. F9 switches it while playing."),
        Item::Orientation => ("Rotation", Value::Text(shown.orientation.name().into()), "Turns the screens, for games played with the console held sideways."),
        Item::IntegerScale => ("Whole-number scaling", Value::On(shown.integer_scale), "Every pixel the same size, which may leave borders around the screens."),
        Item::TopShape => ("Top screen shape", Value::Text(shown.top_shape.name().into()), "Wider shapes need a widescreen code for the game, in its Cheats."),
        Item::Fullscreen => ("Fullscreen", Value::On(shown.fullscreen), "F11 switches it too."),
        Item::ShowFps => ("Show frame rate", Value::On(shown.show_fps), "Shows the frame rate in a corner while playing."),
        Item::Stereo => ("3D mode", Value::Text(shown.stereo.name().into()), "How the 3D effect is shown, for games that have one."),
        Item::Depth => ("3D depth", Value::Range(format!("{}%", shown.depth_3d)), "How strong the 3D effect is."),
        Item::SwapEyes => ("Swap eyes", Value::On(shown.swap_eyes), "Swaps the pictures for the left and right eye."),
        Item::Volume => ("Volume", Value::Range(format!("{:.0}%", shown.volume * 100.0)), ""),
        Item::Mute => ("Mute", Value::On(shown.mute), ""),
        Item::Microphone => ("Microphone", Value::Text(shown.microphone.name().into()), "What games hear. Hold M to blow into it."),
        Item::Language => ("Console language", Value::Text(shown.language.name().into()), "The language games show their text in, when they have it."),
        Item::Recompiled => ("Use recompiled code", Value::On(shown.recompiled), "Runs games from code recompiled for your PC, which is much faster."),
        Item::Camera => ("Cameras see", Value::Text(shown.camera.name().into()), "What the console's cameras see."),
        Item::CameraPicture => ("Camera picture", Value::Text(file(&shown.camera_picture)), "The picture the cameras see, when they see one."),
        Item::BackgroundOpacity => ("Background strength", Value::Range(format!("{:.0}%", shown.background_opacity * 100.0)), ""),
        Item::Background => ("Home background", Value::Text(file(&shown.background)), "A picture behind the Home screen."),
        _ => ("", Value::None, ""),
    };
    // a setting that can't be used now says why
    let (enabled, note) = match item {
        Item::Resolution | Item::TexturePacks | Item::SmoothTextures if !hardware => (false, Some("Needs 3D rendering on the GPU")),
        Item::Interpolation if !hardware || shown.renderer != Renderer::Vulkan => (false, Some("Needs Vulkan and 3D rendering on the GPU")),
        Item::Depth | Item::SwapEyes => (stereo, None),
        Item::BackgroundOpacity => (shown.background.is_some(), None),
        _ => (true, None),
    };
    let mut row = Row::new(item, label, value).enabled(enabled);
    row.note = note.map(str::to_owned);
    row.help = Some(help).filter(|help| !help.is_empty());
    row.changed = owned(settings, scope, item);
    row
}

impl Menus {
    /// whether something is open over the Home screen or the menu.
    pub fn modal_open(&self) -> bool {
        self.settings_open
            || self.cheats_open
            || self.multiplayer_open
            || self.game_settings.is_some()
            || self.message.is_some()
            || self.select_popup.is_some()
            || self.import.is_some()
    }

    /// whether the interface is over a running game, which then gets none
    /// of the input.
    pub fn over_game(&self) -> bool {
        self.menu_open || self.modal_open()
    }

    /// closes what is on top, the list picked from before the window it
    /// came from.
    pub fn close_modals(&mut self) {
        if self.select_popup.take().is_some() {
            return;
        }
        if self.message.take().is_some() {
            self.report = None;
            return;
        }
        self.settings_open = false;
        self.cheats_open = false;
        self.cheat_form = false;
        self.multiplayer_open = false;
        self.game_settings = None;
        self.import = None;
        self.rebinding = None;
        self.rebinding_pad = None;
    }

    pub fn open_settings(&mut self) {
        self.settings_open = true;
        self.game_settings = None;
        self.page = Page { scroll: true, ..Page::default() };
    }

    /// opens a game's options, with its texture packs read from its folder.
    pub fn open_game_settings(&mut self, program_id: u64, name: String, data_dir: Option<&Path>) {
        self.game_settings = Some((program_id, name));
        self.settings_open = false;
        self.page = Page { scroll: true, ..Page::default() };
        self.refresh_packs(data_dir);
    }

    /// reads the texture packs of the game whose options are open again.
    pub fn refresh_packs(&mut self, data_dir: Option<&Path>) {
        self.packs = match (&self.game_settings, data_dir) {
            (Some((id, _)), Some(data_dir)) => store::texture_packs(data_dir, *id),
            _ => Vec::new(),
        };
    }

    /// shows what was found to add, the games each pack or cheat file
    /// likely goes with already ticked, or the game it was added from.
    pub fn open_import(&mut self, found: Vec<Found>, library: &Library, for_game: Option<u64>) {
        let games = playable(library);
        let found = found
            .into_iter()
            .map(|found| {
                let mut wanted = for_game.map_or_else(|| store::likely_games(&found, &games), |id| vec![id]);
                if wanted.is_empty() && games.len() == 1 {
                    wanted = vec![games[0].0];
                }
                let include = !matches!(found, Found::Unusable { .. });
                (found, include, wanted)
            })
            .collect();
        self.import = Some(ImportView { found, place: PackPlace::Copy });
        self.import_row = 0;
    }

    /// a press, done at the next frame, where what is shown is known.
    pub fn press(&mut self, nav: Nav) {
        self.queue.push(nav);
    }

    /// the interface for a frame, with what the player asked for in it.
    pub fn show(&mut self, ui: &mut egui::Ui, settings: &mut Settings, view: &View) -> Vec<Action> {
        let mut actions = Vec::new();
        let before = settings.clone();
        for nav in std::mem::take(&mut self.queue) {
            actions.extend(self.navigate(nav, settings, view));
        }
        if view.library.changed {
            self.icons.clear();
        }
        let ctx = ui.ctx().clone();
        let layered = self.modal_open();
        if view.running.is_none() {
            // Home fades while a window is over it; underneath stays solid,
            // not the black the window is cleared to
            ui.painter().rect_filled(ui.max_rect(), 0.0, color::bg());
        }
        ui.add_enabled_ui(!layered, |ui| match &view.running {
            Some(running) => self.over(ui, running, view),
            None => self.home(ui, settings, view),
        });
        self.fade_from_home(&ctx, view.running.is_some());
        if let Some(text) = view.corner.filter(|_| view.running.is_some()) {
            corner(&ctx, text);
        }
        self.install_progress(&ctx, view);
        let popup = self.select_popup.is_some() || self.message.is_some();
        if self.settings_open || self.game_settings.is_some() {
            self.page_window(&ctx, settings, view, !popup);
        }
        if self.cheats_open {
            self.cheats(&ctx, view, !popup);
        }
        if self.multiplayer_open {
            actions.extend(self.multiplayer(&ctx, settings, view.room, !popup));
        }
        if self.import.is_some() {
            self.import_window(&ctx, view, !popup);
        }
        if let Some(request) = view.keyboard {
            actions.extend(self.keyboard(&ctx, request));
        }
        if self.select_popup.is_some() {
            self.popup(&ctx);
        }
        self.message_window(&ctx);
        for (item, op) in std::mem::take(&mut self.clicked) {
            actions.extend(self.handle(item, op, settings, view));
        }
        if let Some(index) = self.clicked_tile.take() {
            actions.extend(self.tile(index, settings, view));
        }
        if let Some(dock) = self.clicked_dock.take() {
            actions.extend(self.dock_action(dock, settings));
        }
        if let Some((name, code)) = self.pending_cheat.take() {
            actions.push(Action::AddCheat(name, code));
        }
        if std::mem::take(&mut self.pending_cancel_install) {
            actions.push(Action::CancelInstall);
        }
        if *settings != before && !actions.contains(&Action::Settings) {
            actions.push(Action::Settings);
        }
        // the highlight breathes while buttons drive the interface
        if self.mode == InputMode::Directional {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        actions
    }

    /// what a press does, on whatever is on top.
    pub fn navigate(&mut self, nav: Nav, settings: &mut Settings, view: &View) -> Vec<Action> {
        // a binding listening takes its button elsewhere, Back leaves it
        if self.rebinding.is_some() || self.rebinding_pad.is_some() {
            if nav == Nav::Back {
                self.rebinding = None;
                self.rebinding_pad = None;
            }
            return Vec::new();
        }
        if self.message.is_some() {
            if matches!(nav, Nav::Accept | Nav::Back) {
                self.message = None;
                self.report = None;
            }
            return Vec::new();
        }
        if let Some(popup) = &mut self.select_popup {
            let count = popup.options.len().max(1);
            match nav {
                Nav::Up => popup.selected = popup.selected.saturating_sub(1),
                Nav::Down => popup.selected = (popup.selected + 1).min(count - 1),
                Nav::Accept => {
                    let chosen = popup.selected;
                    return self.handle(Item::Choice(chosen), Op::Activate, settings, view);
                }
                Nav::Back => self.select_popup = None,
                _ => {}
            }
            return Vec::new();
        }
        if view.keyboard.is_some() {
            return Vec::new();
        }
        if self.import.is_some() {
            let entries = self.import_entries(view);
            return self.list_nav(nav, &entries, ListOf::Import, settings, view);
        }
        if self.multiplayer_open {
            if nav == Nav::Back {
                self.multiplayer_open = false;
            }
            return Vec::new();
        }
        if self.cheats_open {
            if self.cheat_form {
                if nav == Nav::Back {
                    self.cheat_form = false;
                }
                return Vec::new();
            }
            let entries = self.cheat_entries(view);
            return self.list_nav(nav, &entries, ListOf::Cheats, settings, view);
        }
        if self.settings_open || self.game_settings.is_some() {
            return self.page_nav(nav, settings, view);
        }
        match &view.running {
            Some(running) if self.menu_open => {
                let entries = self.pause_entries(running);
                if nav == Nav::Back {
                    return vec![Action::Resume];
                }
                self.list_nav(nav, &entries, ListOf::Pause, settings, view)
            }
            Some(_) => Vec::new(),
            None => self.home_nav(nav, settings, view),
        }
    }

    /// moves through a list, or does what its row does.
    fn list_nav(&mut self, nav: Nav, entries: &[Entry], list: ListOf, settings: &mut Settings, view: &View) -> Vec<Action> {
        let count = rows(entries).count();
        let row = match list {
            ListOf::Pause => &mut self.pause_row,
            ListOf::Cheats => &mut self.cheats_row,
            ListOf::Import => &mut self.import_row,
            ListOf::Page => &mut self.page.row,
        };
        *row = (*row).min(count.saturating_sub(1));
        let current = rows(entries).nth(*row).cloned();
        match nav {
            Nav::Up => {
                *row = row.saturating_sub(1);
                self.page.scroll = true;
            }
            Nav::Down => {
                *row = (*row + 1).min(count.saturating_sub(1));
                self.page.scroll = true;
            }
            Nav::Left | Nav::Right => {
                let delta = if nav == Nav::Left { -1 } else { 1 };
                if let Some(current) = current.filter(|row| row.enabled) {
                    if !matches!(value_kind(current.item), Kind::Other) {
                        return self.handle(current.item, Op::Step(delta), settings, view);
                    }
                }
                if nav == Nav::Left && list == ListOf::Page {
                    self.page.in_list = false;
                }
            }
            Nav::Accept => {
                if let Some(current) = current.filter(|row| row.enabled) {
                    return self.handle(current.item, Op::Activate, settings, view);
                }
            }
            Nav::X => {
                if let Some(current) = current {
                    return self.handle(current.item, Op::Reset, settings, view);
                }
            }
            Nav::Back => match list {
                ListOf::Page => self.page.in_list = false,
                ListOf::Cheats => self.cheats_open = false,
                ListOf::Import => self.import = None,
                ListOf::Pause => {}
            },
            _ => {}
        }
        Vec::new()
    }

    /// moves through the system settings or a game's options: the sections
    /// down the side, and the list of the one chosen.
    fn page_nav(&mut self, nav: Nav, settings: &mut Settings, view: &View) -> Vec<Action> {
        let sections = self.sections();
        let count = sections.len();
        match nav {
            Nav::PrevTab | Nav::NextTab => {
                let delta = if nav == Nav::PrevTab { count - 1 } else { 1 };
                self.page.section = (self.page.section + delta) % count;
                self.page.row = 0;
                self.page.scroll = true;
                return Vec::new();
            }
            Nav::Start if self.settings_open => {
                self.close_modals();
                return Vec::new();
            }
            _ => {}
        }
        if !self.page.in_list {
            match nav {
                Nav::Up => self.page.section = self.page.section.saturating_sub(1),
                Nav::Down => self.page.section = (self.page.section + 1).min(count - 1),
                Nav::Right | Nav::Accept => {
                    self.page.in_list = true;
                    self.page.row = 0;
                    self.page.scroll = true;
                }
                Nav::Back => self.close_modals(),
                _ => {}
            }
            return Vec::new();
        }
        let entries = self.page_entries(settings, view);
        self.list_nav(nav, &entries, ListOf::Page, settings, view)
    }

    /// moves along the games, and down to the round buttons under them.
    fn home_nav(&mut self, nav: Nav, settings: &mut Settings, view: &View) -> Vec<Action> {
        let tiles = view.library.games.len() + 1;
        match (self.home_focus, nav) {
            (_, Nav::Start) => self.open_settings(),
            (HomeFocus::Games, Nav::Left) => self.selected_game = self.selected_game.saturating_sub(1),
            (HomeFocus::Games, Nav::Right) => self.selected_game = (self.selected_game + 1).min(tiles - 1),
            (HomeFocus::Games, Nav::PrevTab) => self.selected_game = self.selected_game.saturating_sub(5),
            (HomeFocus::Games, Nav::NextTab) => self.selected_game = (self.selected_game + 5).min(tiles - 1),
            (HomeFocus::Games, Nav::Down) => self.home_focus = HomeFocus::Dock,
            (HomeFocus::Games, Nav::Accept) => return self.tile(self.selected_game, settings, view),
            (HomeFocus::Games, Nav::Y) => {
                if let Some(game) = view.library.games.get(self.selected_game).filter(|game| game.problem.is_none()) {
                    self.open_game_settings(game.program_id, game.name.clone(), view.data_dir);
                }
            }
            (HomeFocus::Dock, Nav::Up) => self.home_focus = HomeFocus::Games,
            (HomeFocus::Dock, Nav::Left) => self.dock = self.dock.saturating_sub(1),
            (HomeFocus::Dock, Nav::Right) => self.dock = (self.dock + 1).min(DOCK.len() - 1),
            (HomeFocus::Dock, Nav::Accept) => return self.dock_action(DOCK[self.dock], settings),
            (HomeFocus::Dock, Nav::Back) => self.home_focus = HomeFocus::Games,
            _ => {}
        }
        Vec::new()
    }

    /// what choosing a tile on Home does: plays its game, or adds games.
    fn tile(&mut self, index: usize, settings: &Settings, view: &View) -> Vec<Action> {
        match view.library.games.get(index) {
            Some(game) if Busy::of(game.program_id, view.jobs, view.waiting).is_some() => {
                self.toast(&format!("{} is recompiling. It can be played once that finishes.", game.name));
                Vec::new()
            }
            Some(_) if let Some(name) = crate::recompile::blocking_play(view.jobs, view.waiting) => {
                self.toast(&format!("Games can be started once {name} finishes recompiling, which takes the whole processor."));
                Vec::new()
            }
            Some(game) if game.problem.is_none() && asks_to_recompile(game, settings) => {
                self.open_ask(Ask::Play { game: game.wanted(), name: game.name.clone() }, view.jobs, view.waiting);
                Vec::new()
            }
            Some(game) if game.problem.is_none() => vec![Action::Play(game.path.clone())],
            Some(game) => {
                self.message = Some(format!("{} can't be played. {}.", file_label(&game.path), game.problem.as_deref().unwrap_or("")));
                Vec::new()
            }
            None => vec![Action::AddContent(None)],
        }
    }

    fn dock_action(&mut self, dock: Dock, _settings: &mut Settings) -> Vec<Action> {
        match dock {
            Dock::Add => return vec![Action::AddContent(None)],
            Dock::Multiplayer => self.multiplayer_open = true,
            Dock::Settings => self.open_settings(),
            Dock::Quit => return vec![Action::Quit],
        }
        Vec::new()
    }

    /// the sections of the page open.
    fn sections(&self) -> &'static [Section] {
        if self.game_settings.is_some() { &Section::GAME } else { &Section::SYSTEM }
    }

    /// asks the player about a recompile, saying when another one goes first.
    pub fn open_ask(&mut self, ask: Ask, jobs: &[Job], waiting: &[Wanted]) {
        let (title, options) = ask.popup(crate::recompile::blocking_play(jobs, waiting));
        self.select_popup = Some(SelectPopup { item: Item::Ask, scope: self.scope(), title, options, selected: 0, current: None });
        self.ask = Some(ask);
    }

    fn scope(&self) -> Scope {
        match &self.game_settings {
            Some((id, _)) => Scope::Game(*id),
            None => Scope::System,
        }
    }

    /// the rows of the page's section chosen.
    fn page_entries(&self, settings: &Settings, view: &View) -> Vec<Entry> {
        let scope = self.scope();
        let section = self.sections()[self.page.section.min(self.sections().len() - 1)];
        let mut entries = Vec::new();
        let mut add = |item: Item| entries.push(Entry::Row(setting(item, settings, scope)));
        let game = scope != Scope::System;
        match section {
            Section::Graphics => {
                for item in [Item::Resolution, Item::Filter, Item::SmoothTextures, Item::TexturePacks] {
                    add(item);
                }
                entries.push(Entry::Header("Motion".into()));
                for item in [Item::FpsLimit, Item::Interpolation] {
                    entries.push(Entry::Row(setting(item, settings, scope)));
                }
                entries.push(Entry::Header("Advanced".into()));
                if !game {
                    let mut api = setting(Item::Renderer, settings, scope);
                    if view.running.is_some() {
                        // the running game draws with the presenter's device
                        api = api.enabled(false).note("Change it from Home");
                    }
                    entries.push(Entry::Row(api));
                }
                entries.push(Entry::Row(setting(Item::Rasterizer, settings, scope)));
            }
            Section::Screens => {
                for item in [Item::Layout, Item::Orientation, Item::IntegerScale, Item::TopShape] {
                    add(item);
                }
                if !game {
                    for item in [Item::Fullscreen, Item::ShowFps] {
                        add(item);
                    }
                    add(Item::Theme);
                }
            }
            Section::Stereo => {
                add(Item::Stereo);
                add(Item::Depth);
                if !game {
                    add(Item::SwapEyes);
                }
            }
            Section::Audio => {
                for item in [Item::Volume, Item::Mute, Item::Microphone] {
                    add(item);
                }
            }
            Section::System => {
                add(Item::Language);
                add(Item::Recompiled);
                if !game {
                    add(Item::Camera);
                    add(Item::CameraPicture);
                    if view.running.is_some() {
                        entries.push(Entry::Row(Row::new(Item::CopyReport, "Copy report info", Value::More).note("The game's details and the end of the log")));
                    }
                } else if let Some(&(id, _)) = self.game_settings.as_ref().filter(|_| view.running.is_none()) {
                    // recompiling is started from Home, not over a running game
                    entries.push(Entry::Header("Performance".into()));
                    entries.push(Entry::Row(recompile_row(id, view)));
                }
            }
            Section::Controls => {
                entries.push(Entry::Header("Controller".into()));
                let mut pad = settings.pad.clone();
                for (index, (name, button)) in pad.all_mut().into_iter().enumerate() {
                    let value = if self.rebinding_pad == Some(index) { "Press a button…".to_owned() } else { button_name(*button).to_owned() };
                    entries.push(Entry::Row(Row::new(Item::Pad(index), name, Value::Text(value))));
                }
                entries.push(Entry::Row(Row::new(Item::PadDefaults, "Reset controller buttons", Value::More)));
                entries.push(Entry::Header("Keyboard".into()));
                let mut keys = settings.keys.clone();
                for (index, (name, key)) in keys.all_mut().into_iter().enumerate() {
                    let value = if self.rebinding == Some(index) { "Press a key…".to_owned() } else { key_name(*key) };
                    entries.push(Entry::Row(Row::new(Item::Key(index), name, Value::Text(value))));
                }
                entries.push(Entry::Row(Row::new(Item::KeyDefaults, "Reset keyboard keys", Value::More)));
            }
            Section::Library => {
                entries.push(Entry::Row(Row::new(Item::AddContent, "Add games and content", Value::More).note("Games, updates, DLC, texture packs and cheat files")));
                let outdated = idle(view.library.games.iter().filter(|game| game.outdated).map(Game::wanted), view.jobs, view.waiting).len();
                if outdated > 0 && view.running.is_none() {
                    entries.push(Entry::Row(Row::new(Item::RecompileOutdated, format!("Recompile {outdated} outdated games"), Value::More).note("About ten minutes each, one after another. Games can't be started meanwhile")));
                }
                entries.push(Entry::Header("Home".into()));
                entries.push(Entry::Row(setting(Item::Background, settings, scope)));
                entries.push(Entry::Row(setting(Item::BackgroundOpacity, settings, scope)));
            }
            Section::AddOns => {
                let Some((id, _)) = self.game_settings else { return entries };
                entries.push(Entry::Header("Texture packs".into()));
                for (index, pack) in self.packs.iter().enumerate() {
                    let how = if pack.target.is_some() { "Linked" } else { "In Zakuro" };
                    entries.push(Entry::Row(Row::new(Item::Pack(index), pack.name.clone(), Value::Text(how.into())).note("Select to remove")));
                }
                entries.push(Entry::Row(Row::new(Item::AddPack, "Add a texture pack", Value::More).note("A folder or zip of PNG textures")));
                entries.push(Entry::Header("Cheats".into()));
                let playing = view.running.as_ref().is_some_and(|running| running.program_id == Some(id));
                let cheats = Row::new(Item::Cheats, "Cheats and enhancements", Value::More);
                entries.push(Entry::Row(if playing { cheats } else { cheats.enabled(false).note("Available while the game is running") }));
                entries.push(Entry::Header("Files".into()));
                entries.push(Entry::Row(Row::new(Item::OpenMods, "Open the mods folder", Value::More).note("For romfs mods, code patches and loose textures")));
                entries.push(Entry::Row(Row::new(Item::UseSystem, "Use system settings for everything", Value::More).enabled(settings.has_own(id)).danger()));
                let busy = Busy::of(id, view.jobs, view.waiting).is_some();
                let remove = Row::new(Item::RemoveGame, "Remove this game", Value::More).danger();
                entries.push(Entry::Row(match (playing, busy) {
                    (true, _) => remove.enabled(false).note("Not while it is running"),
                    (_, true) => remove.enabled(false).note("Not while it is recompiling"),
                    _ => remove.note("Takes it off Home. Its saves and mods stay"),
                }));
            }
        }
        // what a game reads as it starts says so when the running game
        // started with something else
        if let Some((running, started)) = view.running.as_ref().and_then(|running| Some((running, running.started?))) {
            let ours = match scope {
                Scope::Game(id) => running.program_id == Some(id),
                Scope::System => true,
            };
            let now = settings.for_game(running.program_id);
            for entry in entries.iter_mut().filter(|_| ours) {
                let Entry::Row(row) = entry else { continue };
                let differs = match row.item {
                    Item::Language => now.language != started.language,
                    Item::Recompiled => now.recompiled != started.recompiled,
                    Item::Rasterizer => now.hardware_rasterizer != started.hardware_rasterizer,
                    _ => false,
                };
                if differs {
                    row.note = Some("Restart the game to apply".into());
                }
            }
        }
        entries
    }

    /// the rows of the menu over a running game.
    fn pause_entries(&self, running: &RunningView) -> Vec<Entry> {
        let slot = format!("Slot {} · {}", self.slot.max(1), running.saved.unwrap_or("empty"));
        let mut entries = vec![
            Entry::Row(Row::new(Item::Resume, "Resume", Value::None)),
            Entry::Row(Row::new(Item::Slot, "State slot", Value::Range(slot))),
            Entry::Row(Row::new(Item::SaveState, "Save state", Value::Text("F5".into()))),
            Entry::Row(Row::new(Item::LoadState, "Load state", Value::Text("F8".into())).enabled(running.saved.is_some())),
        ];
        if running.program_id.is_some() {
            entries.push(Entry::Row(Row::new(Item::Options, "Options for this game", Value::More)));
        }
        entries.push(Entry::Row(Row::new(Item::Cheats, "Cheats", Value::More)));
        entries.push(Entry::Row(Row::new(Item::SystemSettings, "System settings", Value::More)));
        entries.push(Entry::Row(Row::new(Item::Restart, "Restart game", Value::None)));
        entries.push(Entry::Row(Row::new(Item::Home, "Return to Home", Value::None)));
        entries.push(Entry::Row(Row::new(Item::Quit, "Quit Zakuro", Value::None).danger()));
        entries
    }

    /// the rows of the cheats window.
    fn cheat_entries(&self, view: &View) -> Vec<Entry> {
        let mut entries = Vec::new();
        if view.cheats.iter().any(|cheat| cheat.builtin) {
            entries.push(Entry::Header("Enhancements".into()));
            for (index, cheat) in view.cheats.iter().enumerate().filter(|(_, cheat)| cheat.builtin) {
                entries.push(Entry::Row(Row::new(Item::Cheat(index), cheat.name.clone(), Value::On(cheat.enabled)).note(cheat.notes.clone())));
            }
        }
        entries.push(Entry::Header("Your cheats".into()));
        for (index, cheat) in view.cheats.iter().enumerate().filter(|(_, cheat)| !cheat.builtin) {
            let mut row = Row::new(Item::Cheat(index), cheat.name.clone(), Value::On(cheat.enabled));
            if !cheat.notes.is_empty() {
                row = row.note(cheat.notes.lines().next().unwrap_or_default().to_owned());
            }
            entries.push(Entry::Row(row));
        }
        entries.push(Entry::Row(Row::new(Item::CheatAdd, "Add a cheat", Value::More).note("Type a code, or add a Citra cheat file from Home")));
        entries.push(Entry::Row(Row::new(Item::CheatsReload, "Reload from file", Value::None)));
        entries.push(Entry::Row(Row::new(Item::CheatsFolder, "Open the cheats folder", Value::More)));
        entries
    }

    /// the rows of what was found to add.
    fn import_entries(&self, view: &View) -> Vec<Entry> {
        let Some(import) = &self.import else { return Vec::new() };
        let games = playable(view.library);
        let mut entries = Vec::new();
        let mut packs_from_folders = false;
        for (index, (found, include, wanted)) in import.found.iter().enumerate() {
            match found {
                Found::Title { name, kind, size, .. } => {
                    let kind = match kind {
                        zakuro_fs::TitleKind::Game => "Game",
                        zakuro_fs::TitleKind::Update => "Update",
                        zakuro_fs::TitleKind::Dlc => "DLC",
                        zakuro_fs::TitleKind::DlpChild => "Download Play",
                    };
                    let note = format!("{kind} · {} · {}", store::size_name(*size), file_label(found.path()));
                    entries.push(Entry::Row(Row::new(Item::ImportFound(index), name.clone(), Value::On(*include)).note(note)));
                }
                Found::ZippedGames { files, size, .. } => {
                    let note = format!("{files} games in a zip · {}", store::size_name(*size));
                    entries.push(Entry::Row(Row::new(Item::ImportFound(index), file_label(found.path()), Value::On(*include)).note(note)));
                }
                Found::Textures { name, images, size, zip, .. } => {
                    packs_from_folders |= !zip;
                    entries.push(Entry::Header(format!("Texture pack · {name} · {images} textures · {}", store::size_name(*size))));
                    if games.is_empty() {
                        entries.push(Entry::Row(Row::new(Item::ImportCancel, "Add a game first, then its texture pack", Value::None).enabled(false)));
                    }
                    for (id, game) in &games {
                        entries.push(Entry::Row(Row::new(Item::ImportFor(index, *id), format!("Use for {game}"), Value::On(wanted.contains(id)))));
                    }
                }
                Found::Cheats { count, .. } => {
                    entries.push(Entry::Header(format!("Cheats · {} · {count} cheats", file_label(found.path()))));
                    for (id, game) in &games {
                        entries.push(Entry::Row(Row::new(Item::ImportFor(index, *id), format!("Add to {game}"), Value::On(wanted.contains(id)))));
                    }
                }
                Found::Unusable { why, .. } => {
                    entries.push(Entry::Row(Row::new(Item::ImportFound(index), file_label(found.path()), Value::None).note(why.clone()).enabled(false)));
                }
            }
        }
        if packs_from_folders {
            entries.push(Entry::Header("Texture pack storage".into()));
            let place = match import.place {
                PackPlace::Copy => "Copy into Zakuro",
                PackPlace::InPlace => "Use from where it is",
            };
            let note = match import.place {
                PackPlace::Copy => "Keeps working if the original is moved or its drive is gone",
                PackPlace::InPlace => "Takes no extra space; keep the folder where it is",
            };
            entries.push(Entry::Row(Row::new(Item::ImportPlace, "Storage", Value::Range(place.into())).note(note)));
        }
        entries.push(Entry::Header(String::new()));
        let (count, bytes) = import_summary(import, view.data_dir, view.library);
        entries.push(Entry::Row(Row::new(Item::ImportAdd, if count == 1 { "Add 1 item".to_owned() } else { format!("Add {count} items") }, Value::Text(bytes)).enabled(count > 0)));
        entries.push(Entry::Row(Row::new(Item::ImportCancel, "Cancel", Value::None)));
        entries
    }

    /// does what a row does.
    fn handle(&mut self, item: Item, op: Op, settings: &mut Settings, view: &View) -> Vec<Action> {
        let mut actions = Vec::new();
        let scope = self.scope();
        if op == Op::Reset {
            if let Scope::Game(id) = scope {
                if let Some(own) = settings.per_game.get_mut(&game_key(id)) {
                    own_fields!(own, item, |field| {
                        *field = None;
                        true
                    });
                }
                settings.prune();
            }
            return actions;
        }
        // values change the same everywhere
        if let Item::Choice(index) = item {
            match self.select_popup.take() {
                Some(SelectPopup { item: Item::Ask, .. }) => {
                    if let Some(ask) = self.ask.take() {
                        actions.extend(ask.answer(index, settings));
                    }
                }
                Some(SelectPopup { item: Item::RemoveGame, .. }) => {
                    if let Some(id) = self.game_settings.as_ref().map(|(id, _)| *id).filter(|_| index == 0) {
                        actions.push(Action::RemoveGame(id));
                    }
                }
                Some(SelectPopup { item: Item::Pack(pack), .. }) => {
                    if let Some(pack) = self.packs.get(pack).filter(|_| index == 0) {
                        actions.push(Action::RemovePack(pack.clone()));
                    }
                }
                Some(popup) => choose(popup.item, index, settings, popup.scope),
                None => {}
            }
            return actions;
        }
        match (value_kind(item), op) {
            (Kind::Choice, Op::Activate) => {
                let shown = settings.for_game(match scope {
                    Scope::Game(id) => Some(id),
                    Scope::System => None,
                });
                if let Some((options, selected)) = choices(item, &shown) {
                    let title = setting(item, settings, scope).label;
                    self.select_popup = Some(SelectPopup { item, scope, title, options, selected, current: Some(selected) });
                }
                return actions;
            }
            (Kind::Switch, Op::Activate) => {
                toggle(item, settings, scope);
                return actions;
            }
            (Kind::Choice | Kind::Switch | Kind::Range, Op::Step(delta)) if item != Item::Slot => {
                step(item, delta, settings, scope);
                return actions;
            }
            _ => {}
        }
        match item {
            Item::Fullscreen => actions.push(Action::Fullscreen),
            Item::Volume | Item::Depth | Item::BackgroundOpacity => {
                if op == Op::Activate {
                    step(item, 1, settings, scope);
                }
            }
            Item::Pad(index) => {
                self.rebinding_pad = Some(index);
                self.rebinding = None;
            }
            Item::Key(index) => {
                self.rebinding = Some(index);
                self.rebinding_pad = None;
            }
            Item::PadDefaults => {
                settings.pad = PadButtons::default();
                self.toast("Controller buttons reset");
            }
            Item::KeyDefaults => {
                settings.keys = Keys::default();
                self.toast("Keyboard keys reset");
            }
            Item::AddContent => actions.push(Action::AddContent(None)),
            Item::RecompileOutdated if view.running.is_none() => {
                let outdated = idle(view.library.games.iter().filter(|game| game.outdated).map(Game::wanted), view.jobs, view.waiting);
                actions.push(Action::Recompile(outdated));
            }
            Item::Background => {
                if settings.background.is_some() && op == Op::Activate {
                    settings.background = None;
                } else {
                    actions.push(Action::ChooseBackground);
                }
            }
            Item::CameraPicture => actions.push(Action::ChooseCameraPicture),
            Item::CopyReport => actions.push(Action::CopyReport),
            Item::Pack(index) => {
                // a pack kept in Zakuro goes for good, so it is asked first
                if let Some(pack) = self.packs.get(index) {
                    let title = match pack.target {
                        Some(_) => format!("Remove {}? The pack stays where it is.", pack.name),
                        None => format!("Delete {}? Its files are deleted.", pack.name),
                    };
                    let options = vec![if pack.target.is_some() { "Remove" } else { "Delete" }.to_owned(), "Keep".to_owned()];
                    self.select_popup = Some(SelectPopup { item, scope, title, options, selected: 1, current: None });
                }
            }
            Item::AddPack => {
                if let Some((id, _)) = self.game_settings {
                    actions.push(Action::AddTextures(id));
                }
            }
            Item::Cheats => {
                self.cheats_open = true;
                self.cheats_row = 0;
            }
            Item::OpenMods => {
                if let Some((id, _)) = self.game_settings {
                    actions.push(Action::Mods(id));
                }
            }
            Item::Recompile if view.running.is_none() => {
                if let Some((id, _)) = self.game_settings {
                    let busy = view.jobs.iter().any(|job| job.program_id == id && !job.stage().finished()) || view.waiting.iter().any(|w| w.program_id == id);
                    if busy {
                        actions.push(Action::CancelRecompile(id));
                    } else if let Some(game) = view.library.games.iter().find(|game| game.program_id == id && game.problem.is_none()) {
                        self.open_ask(Ask::Explain { game: game.wanted(), name: game.name.clone() }, view.jobs, view.waiting);
                    }
                }
            }
            Item::RemoveGame => {
                // removing deletes the copies in Zakuro's library, so it is asked first
                if let Some((_, name)) = &self.game_settings {
                    let title = format!("Delete {name}, with its update and DLC, from Zakuro? Its saves and mods are kept.");
                    self.select_popup = Some(SelectPopup { item, scope, title, options: vec!["Delete".to_owned(), "Keep".to_owned()], selected: 1, current: None });
                }
            }
            Item::UseSystem => {
                if let Some((id, name)) = &self.game_settings {
                    settings.per_game.remove(&game_key(*id));
                    let text = format!("{name} now uses the system settings");
                    self.toast(&text);
                }
            }
            Item::Resume => actions.push(Action::Resume),
            Item::Slot => {
                let delta = match op {
                    Op::Step(delta) => delta,
                    _ => 1,
                };
                self.slot = ((self.slot.max(1) as i32 - 1 + delta).rem_euclid(9) + 1) as u8;
            }
            Item::SaveState => actions.push(Action::SaveState),
            Item::LoadState => actions.push(Action::LoadState),
            Item::Options => {
                if let Some(running) = &view.running {
                    if let Some(id) = running.program_id {
                        self.open_game_settings(id, running.name.to_owned(), view.data_dir);
                    }
                }
            }
            Item::SystemSettings => self.open_settings(),
            Item::Restart => actions.push(Action::Reset),
            Item::Home => actions.push(Action::Library),
            Item::Quit => actions.push(Action::Quit),
            Item::ImportFound(index) => {
                if let Some((_, include, _)) = self.import.as_mut().and_then(|import| import.found.get_mut(index)) {
                    *include = !*include;
                }
            }
            Item::ImportFor(index, id) => {
                if let Some((_, include, wanted)) = self.import.as_mut().and_then(|import| import.found.get_mut(index)) {
                    match wanted.iter().position(|&game| game == id) {
                        Some(at) => {
                            wanted.remove(at);
                        }
                        None => wanted.push(id),
                    }
                    *include = !wanted.is_empty();
                }
            }
            Item::ImportPlace => {
                if let Some(import) = &mut self.import {
                    import.place = match import.place {
                        PackPlace::Copy => PackPlace::InPlace,
                        PackPlace::InPlace => PackPlace::Copy,
                    };
                }
            }
            Item::ImportAdd => {
                if let Some(import) = self.import.take() {
                    let found = import
                        .found
                        .into_iter()
                        .filter(|(found, include, wanted)| *include && (!needs_games(found) || !wanted.is_empty()))
                        .map(|(found, _, wanted)| (found, wanted))
                        .collect();
                    actions.push(Action::Install(store::Choice { found, place: import.place }));
                }
            }
            Item::ImportCancel => self.import = None,
            Item::Cheat(index) => {
                if let Some(cheat) = view.cheats.get(index) {
                    actions.push(Action::Cheat(index, !cheat.enabled));
                }
            }
            Item::CheatAdd => self.cheat_form = true,
            Item::CheatsReload => actions.push(Action::ReloadCheats),
            Item::CheatsFolder => actions.push(Action::CheatsFolder),
            _ => {}
        }
        actions
    }

    /// something said for a moment.
    pub fn toast(&mut self, text: &str) {
        self.toast = Some((text.to_owned(), std::time::Instant::now()));
    }
}

/// which list a press moves through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListOf {
    Pause,
    Cheats,
    Import,
    Page,
}

/// whether a thing found goes to games the player picks.
fn needs_games(found: &Found) -> bool {
    matches!(found, Found::Textures { .. } | Found::Cheats { .. })
}

/// the games that can be played, by id and name, each once.
fn playable(library: &Library) -> Vec<(u64, String)> {
    let mut games: Vec<(u64, String)> = Vec::new();
    for game in library.games.iter().filter(|game| game.problem.is_none()) {
        if !games.iter().any(|(id, _)| *id == game.program_id) {
            games.push((game.program_id, game.name.clone()));
        }
    }
    games
}

/// how many things will be added, and the size of what is copied.
fn import_summary(import: &ImportView, data_dir: Option<&Path>, library: &Library) -> (usize, String) {
    let found: Vec<(Found, Vec<u64>)> = import
        .found
        .iter()
        .filter(|(found, include, wanted)| *include && !matches!(found, Found::Unusable { .. }) && (!needs_games(found) || !wanted.is_empty()))
        .map(|(found, _, wanted)| (found.clone(), wanted.clone()))
        .collect();
    let count = found.len();
    let choice = store::Choice { found, place: import.place };
    let _ = library;
    let bytes = match data_dir {
        Some(data_dir) => store::plan(&choice, &data_dir.join("library"), data_dir).1,
        None => 0,
    };
    (count, if bytes > 0 { format!("{} to copy", store::size_name(bytes)) } else { String::new() })
}


/// whether starting game asks about recompiling it first: its code isn't
/// recompiled or is outdated, recompiled code is in use, and the player
/// didn't say to stop asking.
pub fn asks_to_recompile(game: &Game, settings: &Settings) -> bool {
    settings.recompiled && (!game.recompiled || game.outdated) && !settings.dont_ask_recompile.contains(&game_key(game.program_id))
}
/// the row that recompiles a game, with how that is going.
fn recompile_row(id: u64, view: &View) -> Row {
    let game = view.library.games.iter().find(|game| game.program_id == id);
    let job = view.jobs.iter().find(|job| job.program_id == id && !job.stage().finished());
    let waiting = view.waiting.iter().any(|wanted| wanted.program_id == id);
    let (label, value) = match (job, waiting, game) {
        (Some(job), _, _) => (
            "Recompiling — select to stop",
            Busy::Recompiling { stage: job.stage(), compiling: job.compiling_for() }.describe(),
        ),
        (None, true, _) => ("Waiting to recompile — select to stop", String::new()),
        (None, false, Some(game)) if game.outdated => ("Recompile again", "Outdated".into()),
        (None, false, Some(game)) if game.recompiled => ("Recompile again", "Recompiled".into()),
        _ => ("Recompile for speed", "Not recompiled".into()),
    };
    let failed = view.jobs.iter().find(|job| job.program_id == id).and_then(|job| match job.stage() {
        Stage::Failed(error) => Some(format!("Last try failed: {error}")),
        _ => None,
    });
    let row = Row::new(Item::Recompile, label, Value::Text(value)).enabled(game.is_some());
    row.note(failed.unwrap_or_else(|| "Turns the game's code into native code. About ten minutes, using the compiler that comes with Zakuro".into()))
}

/// how a game's recompile is going, when it is recompiling or waiting to,
/// in which case it can't be played.
#[derive(Debug, Clone, PartialEq)]
pub enum Busy {
    Waiting,
    /// compiling, how long the C has been compiling.
    Recompiling { stage: Stage, compiling: Option<std::time::Duration> },
}

impl Busy {
    pub fn of(id: u64, jobs: &[Job], waiting: &[Wanted]) -> Option<Busy> {
        if let Some(job) = jobs.iter().find(|job| job.program_id == id && !job.stage().finished()) {
            return Some(Busy::Recompiling { stage: job.stage(), compiling: job.compiling_for() });
        }
        waiting.iter().any(|wanted| wanted.program_id == id).then_some(Busy::Waiting)
    }

    pub fn fraction(&self) -> f32 {
        match self {
            Busy::Waiting => 0.0,
            Busy::Recompiling { stage, .. } => stage.fraction(),
        }
    }

    /// the stage, how far along, and about how long is left once there is
    /// enough to go by.
    pub fn describe(&self) -> String {
        match self {
            Busy::Waiting => "Waiting to recompile".to_owned(),
            Busy::Recompiling { stage, compiling } => {
                let fraction = stage.fraction();
                let mut text = format!("{}  ·  {:.0}%", stage_name(stage), fraction * 100.0);
                // the files compiled so far, at the pace they went, after
                // enough of them for the pace to mean something
                if let (Stage::Compiling { done, total }, Some(compiling)) = (stage, compiling) {
                    if *done >= 8 && compiling.as_secs() >= 15 {
                        let left = compiling.as_secs_f32() * total.saturating_sub(*done) as f32 / *done as f32;
                        text.push_str(&format!("  ·  about {} left", duration_name(left)));
                    }
                }
                text
            }
        }
    }
}

/// a span of seconds, roughly, in words.
fn duration_name(seconds: f32) -> String {
    let seconds = seconds.round() as u64;
    match seconds {
        0..60 => "under a minute".to_owned(),
        60..3600 => {
            let minutes = (seconds + 30) / 60;
            if minutes == 1 { "1 minute".to_owned() } else { format!("{minutes} minutes") }
        }
        _ => format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
    }
}

fn stage_name(stage: &Stage) -> String {
    match stage {
        Stage::Generating => "Finding code".to_owned(),
        Stage::Compiling { done, total } => format!("Compiling {done}/{total}"),
        Stage::Installing => "Installing".to_owned(),
        Stage::Done => "Done".to_owned(),
        Stage::Failed(_) => "Failed".to_owned(),
    }
}

fn file_label(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

// drawing

/// how a list is drawn.
struct ListStyle {
    row_height: f32,
    /// the selection shows, the list having it.
    focused: bool,
}

impl Menus {
    /// a list of rows, the one selected highlighted, clicks and hovers
    /// moving the selection and clicks doing what the row does.
    fn draw_list(&mut self, ui: &mut egui::Ui, entries: &[Entry], list: ListOf, style: ListStyle) {
        let mode = self.mode;
        let selected = match list {
            ListOf::Pause => self.pause_row,
            ListOf::Cheats => self.cheats_row,
            ListOf::Import => self.import_row,
            ListOf::Page => self.page.row,
        };
        let scroll = std::mem::take(&mut self.page.scroll);
        let time = ui.ctx().input(|input| input.time);
        let mut hovered = None;
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut index = 0;
        let width = ui.available_width();
        let highlight = Highlight::under(ui);
        let mut target = None;
        for entry in entries {
            match entry {
                Entry::Header(text) => {
                    ui.add_space(if text.is_empty() { 6.0 } else { 14.0 });
                    if !text.is_empty() {
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
                        ui.painter().text(rect.left_center() + Vec2::new(14.0, 0.0), Align2::LEFT_CENTER, text, FontId::proportional(13.0), color::muted());
                    }
                }
                Entry::Row(row) => {
                    let height = if row.note.is_some() { style.row_height + 16.0 } else { style.row_height };
                    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
                    let is_selected = index == selected && style.focused;
                    let pointer_over = response.hovered() && ui.is_enabled();
                    if pointer_over && mode == InputMode::Pointer {
                        hovered = Some(index);
                    }
                    if is_selected {
                        target = Some(rect);
                        if scroll {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                    let response = if row.enabled { response.on_hover_cursor(CursorIcon::PointingHand) } else { response };
                    let arrows = paint_row(ui, rect, row, is_selected, mode);
                    if response.clicked() && row.enabled {
                        // a click on a value's arrows steps it
                        let op = match (arrows, response.interact_pointer_pos()) {
                            (Some((less, _)), Some(at)) if less.contains(at) => Op::Step(-1),
                            (Some((_, more)), Some(at)) if more.contains(at) => Op::Step(1),
                            _ => Op::Activate,
                        };
                        self.clicked.push((row.item, op));
                    }
                    if response.secondary_clicked() && row.changed {
                        self.clicked.push((row.item, Op::Reset));
                    }
                    index += 1;
                }
            }
        }
        highlight.paint(ui, target, mode, time);
        if let Some(hovered) = hovered {
            match list {
                ListOf::Pause => self.pause_row = hovered,
                ListOf::Cheats => self.cheats_row = hovered,
                ListOf::Import => self.import_row = hovered,
                ListOf::Page => {
                    self.page.row = hovered;
                    self.page.in_list = true;
                }
            }
        }
    }
}

/// how long the highlight takes to glide to another row, in seconds.
const GLIDE: f32 = 0.11;

/// the selected row's highlight, which goes under a list's rows and glides
/// from row to row rather than jumping.
struct Highlight {
    slot: egui::layers::ShapeIdx,
    /// the list's top, which the highlight's place is measured from, so
    /// that scrolling moves it with the rows at once.
    origin: f32,
}

impl Highlight {
    /// a place for it under what is drawn next.
    fn under(ui: &egui::Ui) -> Highlight {
        Highlight { slot: ui.painter().add(egui::Shape::Noop), origin: ui.cursor().top() }
    }

    /// draws it gliding toward target, the selected row, if one is.
    fn paint(self, ui: &egui::Ui, target: Option<Rect>, mode: InputMode, time: f64) {
        let Some(rect) = target else { return };
        let ctx = ui.ctx();
        let id = ui.id().with("highlight");
        // under the pointer it is at the row at once: gliding after a moving
        // pointer left it trailing over the rows between, which flickered
        let glide = if mode == InputMode::Pointer { 0.0 } else { GLIDE };
        let top = ctx.animate_value_with_time(id.with("top"), rect.top() - self.origin, glide);
        let height = ctx.animate_value_with_time(id.with("height"), rect.height(), glide);
        let inner = Rect::from_min_size(Pos2::new(rect.left(), self.origin + top), Vec2::new(rect.width(), height)).shrink2(Vec2::splat(2.0));
        let mut shapes = vec![egui::Shape::rect_filled(inner, CornerRadius::same(10), color::raised())];
        if mode == InputMode::Directional {
            // a highlight that breathes, as a console's does
            let glow = (0.75 + 0.25 * (time * 3.2).sin()) as f32;
            shapes.push(egui::Shape::rect_stroke(inner, CornerRadius::same(10), Stroke::new(2.5, color::accent().gamma_multiply(glow)), StrokeKind::Inside));
        }
        ui.painter().set(self.slot, egui::Shape::Vec(shapes));
    }
}

/// draws a row: its label, the dot of a game's own value, and its value.
/// the selected row's highlight is drawn by its list, which glides it.
/// a range's arrows are given back, (less, more), where a click steps it.
fn paint_row(ui: &egui::Ui, rect: Rect, row: &Row, selected: bool, mode: InputMode) -> Option<(Rect, Rect)> {
    let mut arrows = None;
    let painter = ui.painter();
    let hovered = ui.rect_contains_pointer(rect) && mode == InputMode::Pointer && row.enabled && ui.is_enabled();
    if hovered && !selected {
        painter.rect_filled(rect.shrink2(Vec2::new(2.0, 2.0)), CornerRadius::same(10), color::raised());
    }
    if !selected && !hovered {
        painter.line_segment([Pos2::new(rect.left() + 14.0, rect.bottom()), Pos2::new(rect.right() - 14.0, rect.bottom())], Stroke::new(1.0, color::line()));
    }
    let text_color = match (row.enabled, row.danger) {
        (false, _) => color::faint(),
        (true, true) => color::danger(),
        (true, false) => color::text(),
    };
    let left = rect.left() + if row.changed { 30.0 } else { 18.0 };
    let label_y = if row.note.is_some() { rect.center().y - 9.0 } else { rect.center().y };
    if row.changed {
        painter.circle_filled(Pos2::new(rect.left() + 17.0, label_y), 4.0, color::accent());
    }
    // the value first, so the label knows the room it has
    let right = rect.right() - 18.0;
    let value_color = if row.enabled { if selected { color::accent() } else { color::muted() } } else { color::faint() };
    let value_left = match &row.value {
        Value::None => right,
        Value::On(on) => {
            let track = Rect::from_center_size(Pos2::new(right - 22.0, rect.center().y), Vec2::new(44.0, 24.0));
            let fill = if *on { color::on() } else { color::track() };
            painter.rect_filled(track, CornerRadius::same(12), if row.enabled { fill } else { fill.gamma_multiply(0.4) });
            let knob = if *on { track.right() - 12.0 } else { track.left() + 12.0 };
            painter.circle_filled(Pos2::new(knob, track.center().y), 9.0, Color32::WHITE);
            track.left()
        }
        Value::Text(text) => painter.text(Pos2::new(right, rect.center().y), Align2::RIGHT_CENTER, text, FontId::proportional(15.0), value_color).left(),
        Value::Range(text) => {
            let shown = painter.text(Pos2::new(right - 26.0, rect.center().y), Align2::RIGHT_CENTER, text, FontId::proportional(15.0), value_color);
            // generous targets, the row's height, for fingers as well
            let less = Rect::from_x_y_ranges(shown.left() - 40.0..=shown.left() - 2.0, rect.y_range());
            let more = Rect::from_x_y_ranges(right - 24.0..=rect.right(), rect.y_range());
            arrows = Some((less, more));
            if selected || hovered {
                crate::icons::paint(painter, Rect::from_center_size(Pos2::new(right - 9.0, rect.center().y), Vec2::splat(18.0)), Icon::ChevronRight, value_color);
                crate::icons::paint(painter, Rect::from_center_size(Pos2::new(shown.left() - 17.0, rect.center().y), Vec2::splat(18.0)), Icon::ChevronLeft, value_color);
                shown.left() - 30.0
            } else {
                shown.left()
            }
        }
        Value::More => {
            let at = Rect::from_center_size(Pos2::new(right - 9.0, rect.center().y), Vec2::splat(18.0));
            crate::icons::paint(painter, at, Icon::ChevronRight, if row.enabled { color::faint() } else { value_color });
            at.left()
        }
        Value::Check => {
            let at = Rect::from_center_size(Pos2::new(right - 10.0, rect.center().y), Vec2::splat(20.0));
            crate::icons::paint(painter, at, Icon::Check, color::accent());
            at.left()
        }
    };
    let room = (value_left - left - 16.0).max(40.0);
    let label = painter.layout(row.label.clone(), FontId::proportional(16.0), text_color, room);
    let label = if label.rows.len() > 1 { painter.layout_no_wrap(truncate(&row.label, room, 16.0), FontId::proportional(16.0), text_color) } else { label };
    painter.galley(Pos2::new(left, label_y - label.size().y / 2.0), label, text_color);
    if let Some(note) = &row.note {
        let note = painter.layout_no_wrap(truncate(note, room, 12.5), FontId::proportional(12.5), color::faint());
        painter.galley(Pos2::new(left, rect.center().y + 3.0), note, color::faint());
    }
    arrows
}

/// how long a window takes to grow in, in seconds.
const POP_IN: f64 = 0.18;

/// grows the window on layer from a little smaller when it comes up,
/// about center. egui fades it in by itself.
fn pop_in(ctx: &egui::Context, layer: egui::LayerId, center: Pos2) {
    let now = ctx.input(|input| input.time);
    let key = layer.id.with("pop in");
    // (when it was last drawn, when it came up)
    let (seen, opened) = ctx.data(|data| data.get_temp::<(f64, f64)>(key)).unwrap_or((f64::MIN, now));
    let opened = if now - seen > 0.25 { now } else { opened };
    ctx.data_mut(|data| data.insert_temp(key, (now, opened)));
    let t = ((now - opened) / POP_IN).clamp(0.0, 1.0) as f32;
    let scale = 0.94 + 0.06 * egui::emath::easing::cubic_out(t);
    if t < 1.0 {
        ctx.request_repaint();
    }
    let transform = egui::emath::TSTransform::from_translation(center.to_vec2() * (1.0 - scale)) * egui::emath::TSTransform::from_scaling(scale);
    ctx.set_transform_layer(layer, transform);
}

/// the semibold weight titles are drawn in.
fn strong(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name(crate::gui::STRONG.into()))
}

/// text cut to about the room, by an average letter's width.
fn truncate(text: &str, room: f32, size: f32) -> String {
    let fits = (room / (size * 0.5)).max(4.0) as usize;
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= fits { text.to_owned() } else { format!("{}…", chars[..fits - 1].iter().collect::<String>()) }
}

/// a round close button in a window's corner, true when clicked.
fn close_button(ui: &mut egui::Ui, center: Pos2) -> bool {
    let rect = Rect::from_center_size(center, Vec2::splat(36.0));
    let response = ui.interact(rect, ui.id().with("close"), Sense::click()).on_hover_cursor(CursorIcon::PointingHand).on_hover_text("Close");
    let fill = if response.hovered() { color::hover() } else { color::raised() };
    ui.painter().circle_filled(center, 18.0, fill);
    crate::icons::paint(ui.painter(), Rect::from_center_size(center, Vec2::splat(18.0)), Icon::Close, color::text());
    response.clicked()
}

/// the hints of what the buttons do, with the keyboard's keys when it was
/// the keyboard.
fn hints(ui: &egui::Ui, rect: Rect, pad: bool, list: &[(Nav, &str)]) {
    let painter = ui.painter();
    let mut x = rect.right() - 20.0;
    // a press the keyboard has no key for goes unhinted
    for (nav, label) in list.iter().rev().filter(|(nav, _)| !glyph(*nav, pad).is_empty()) {
        let text = painter.text(Pos2::new(x, rect.center().y), Align2::RIGHT_CENTER, label, FontId::proportional(14.0), color::muted());
        x = text.left() - 8.0;
        let glyph = glyph(*nav, pad);
        let width = (glyph.chars().count() as f32 * 8.0 + 14.0).max(24.0);
        let badge = Rect::from_center_size(Pos2::new(x - width / 2.0, rect.center().y), Vec2::new(width, 24.0));
        painter.rect_filled(badge, CornerRadius::same(12), color::text());
        painter.text(badge.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(12.5), color::bg());
        x = badge.left() - 20.0;
    }
}

fn glyph(nav: Nav, pad: bool) -> &'static str {
    match (nav, pad) {
        (Nav::Accept, true) => "A",
        (Nav::Back, true) => "B",
        (Nav::X, true) => "X",
        (Nav::Y, true) => "Y",
        (Nav::PrevTab | Nav::NextTab, true) => "LB RB",
        (Nav::Start, true) => "☰",
        (Nav::Accept, false) => "Enter",
        (Nav::Back, false) => "Esc",
        (Nav::X, false) => "X",
        (Nav::Y, false) => "Y",
        (Nav::PrevTab | Nav::NextTab, false) => "Q E",
        (Nav::Start, false) => "",
        (Nav::Left | Nav::Right, _) => "◀ ▶",
        (Nav::Up | Nav::Down, _) => "▲ ▼",
    }
}

/// a window over everything: the screen dimmed behind it, the window in
/// the middle at most as big as asked, its title, a close button, and the
/// hints along its foot. contents draws inside, and true comes back when it
/// was closed.
/// the hints along a window's foot: whether they show, whether for a
/// controller, and which.
type Footer<'a> = Option<(bool, bool, &'a [(Nav, &'a str)])>;

fn window(ctx: &egui::Context, id: &str, size: Vec2, (title, subtitle): (&str, Option<&str>), enabled: bool, footer: Footer, contents: impl FnOnce(&mut egui::Ui)) -> bool {
    let screen = ctx.content_rect();
    let size = Vec2::new(size.x.min(screen.width() - 24.0), size.y.min(screen.height() - 24.0));
    let rect = Rect::from_center_size(screen.center(), size);
    let mut closed = false;
    pop_in(ctx, egui::LayerId::new(egui::Order::Foreground, egui::Id::new(id)), screen.center());
    // the backdrop reaches past the screen, so it still covers it while
    // the window grows in
    let cover = screen.expand(screen.size().max_elem() * 0.1);
    egui::Area::new(egui::Id::new(id))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .movable(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(cover);
            // the area covers the screen, so egui finds it under the pointer and
            // its lists take the mouse wheel
            ui.expand_to_include_rect(screen);
            // the backdrop takes the clicks around the window
            let backdrop = ui.interact(screen, ui.id().with("backdrop"), Sense::click());
            ui.painter().rect_filled(cover, 0.0, color::scrim());
            ui.painter().rect_filled(rect, CornerRadius::same(18), color::panel());
            ui.painter().rect_stroke(rect, CornerRadius::same(18), Stroke::new(1.0, color::line()), StrokeKind::Inside);
            let _ = backdrop;
            let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            ui.add_enabled_ui(enabled, |ui| {
                let header = Rect::from_min_size(rect.min, Vec2::new(rect.width(), if subtitle.is_some() { 76.0 } else { 64.0 }));
                ui.painter().text(header.left_top() + Vec2::new(28.0, 22.0), Align2::LEFT_TOP, title, strong(22.0), color::text());
                if let Some(subtitle) = subtitle {
                    ui.painter().text(header.left_top() + Vec2::new(28.0, 50.0), Align2::LEFT_TOP, subtitle, FontId::proportional(13.5), color::muted());
                }
                if close_button(ui, Pos2::new(header.right() - 34.0, header.top() + 34.0)) {
                    closed = true;
                }
                let foot = if footer.is_some_and(|(show, _, _)| show) { 52.0 } else { 16.0 };
                let body = Rect::from_min_max(Pos2::new(rect.left(), header.bottom()), Pos2::new(rect.right(), rect.bottom() - foot));
                ui.painter().line_segment([body.left_top() + Vec2::new(20.0, 0.0), body.right_top() - Vec2::new(20.0, 0.0)], Stroke::new(1.0, color::line()));
                let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink2(Vec2::new(12.0, 8.0))));
                inner.set_clip_rect(body);
                contents(&mut inner);
                if let Some((true, pad, list)) = footer {
                    let foot = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - foot), rect.max);
                    ui.painter().line_segment([foot.left_top() + Vec2::new(20.0, 0.0), foot.right_top() - Vec2::new(20.0, 0.0)], Stroke::new(1.0, color::line()));
                    hints(ui, foot, pad, list);
                }
            });
        });
    closed
}

impl Menus {
    /// the system settings or a game's options: sections down the side,
    /// the chosen one's rows beside them.
    fn page_window(&mut self, ctx: &egui::Context, settings: &mut Settings, view: &View, enabled: bool) {
        let sections = self.sections();
        self.page.section = self.page.section.min(sections.len() - 1);
        let entries = self.page_entries(settings, view);
        let (title, subtitle) = match &self.game_settings {
            Some((_, name)) => (format!("Options · {name}"), Some("Changes here apply to this game only. A dot marks a change; X or right-click resets it.".to_owned())),
            None => ("System Settings".to_owned(), None),
        };
        let directional = self.mode == InputMode::Directional;
        let mut footer: Vec<(Nav, &str)> = vec![(Nav::PrevTab, "Section"), (Nav::Accept, "Select"), (Nav::Back, "Back")];
        if self.game_settings.is_some() {
            footer.insert(2, (Nav::X, "Reset"));
        }
        let pad = self.pad_hints;
        let mut section_clicked = None;
        let closed = window(ctx, "page", Vec2::new(1080.0, 720.0), (&title, subtitle.as_deref()), enabled, Some((directional, pad, &footer)), |ui| {
            let body = ui.max_rect();
            let side_width = if body.width() > 700.0 { 230.0 } else { 170.0 };
            let side = Rect::from_min_size(body.min, Vec2::new(side_width, body.height()));
            let main = Rect::from_min_max(Pos2::new(side.right() + 12.0, body.top()), body.max);
            let mut side_ui = ui.new_child(egui::UiBuilder::new().max_rect(side));
            for (index, section) in sections.iter().enumerate() {
                let (rect, response) = side_ui.allocate_exact_size(Vec2::new(side_width, 46.0), Sense::click());
                let response = response.on_hover_cursor(CursorIcon::PointingHand);
                let current = index == self.page.section;
                let hovered = response.hovered() && self.mode == InputMode::Pointer;
                if current || hovered {
                    side_ui.painter().rect_filled(rect.shrink(2.0), CornerRadius::same(10), if current { color::raised() } else { color::surface() });
                }
                if current {
                    let bar = Rect::from_min_size(rect.left_top() + Vec2::new(6.0, 12.0), Vec2::new(4.0, 22.0));
                    side_ui.painter().rect_filled(bar, CornerRadius::same(2), color::accent());
                }
                if current && !self.page.in_list && self.mode == InputMode::Directional {
                    let glow = (0.75 + 0.25 * (side_ui.ctx().input(|i| i.time) * 3.2).sin()) as f32;
                    side_ui.painter().rect_stroke(rect.shrink(2.0), CornerRadius::same(10), Stroke::new(2.5, color::accent().gamma_multiply(glow)), StrokeKind::Inside);
                }
                let text_color = if current { color::text() } else { color::muted() };
                let icon = Rect::from_center_size(rect.left_center() + Vec2::new(32.0, 0.0), Vec2::splat(20.0));
                crate::icons::paint(side_ui.painter(), icon, section.icon(), if current { color::accent() } else { color::muted() });
                side_ui.painter().text(rect.left_center() + Vec2::new(54.0, 0.0), Align2::LEFT_CENTER, section.name(), FontId::proportional(16.0), text_color);
                if response.clicked() {
                    section_clicked = Some(index);
                }
            }
            ui.painter().line_segment([Pos2::new(side.right() + 4.0, body.top()), Pos2::new(side.right() + 4.0, body.bottom())], Stroke::new(1.0, color::line()));
            // what the selected setting does, under the list, so the rows
            // stay one line each
            let help = rows(&entries).nth(self.page.row).filter(|_| self.page.in_list || self.mode == InputMode::Pointer).and_then(|row| row.help);
            let has_help = rows(&entries).any(|row| row.help.is_some());
            let main = if has_help {
                let foot = Rect::from_min_max(Pos2::new(main.left(), main.bottom() - 58.0), main.max);
                ui.painter().line_segment([foot.left_top() + Vec2::new(14.0, 0.0), foot.right_top() - Vec2::new(14.0, 0.0)], Stroke::new(1.0, color::line()));
                if let Some(help) = help {
                    let text = ui.painter().layout(help.to_owned(), FontId::proportional(14.0), color::muted(), foot.width() - 36.0);
                    ui.painter().galley(Pos2::new(foot.left() + 18.0, foot.center().y - text.size().y / 2.0), text, color::muted());
                }
                Rect::from_min_max(main.min, Pos2::new(main.right(), foot.top()))
            } else {
                main
            };
            let mut main_ui = ui.new_child(egui::UiBuilder::new().max_rect(main));
            egui::ScrollArea::vertical().id_salt(("page", self.page.section, self.game_settings.is_some())).auto_shrink(false).show(&mut main_ui, |ui| {
                let focused = self.page.in_list || self.mode == InputMode::Pointer;
                self.draw_list(ui, &entries, ListOf::Page, ListStyle { row_height: 52.0, focused });
                ui.add_space(12.0);
            });
        });
        if let Some(index) = section_clicked {
            self.page.section = index;
            self.page.row = 0;
            self.page.in_list = false;
        }
        if closed {
            self.settings_open = false;
            self.game_settings = None;
            self.rebinding = None;
            self.rebinding_pad = None;
        }
    }

    /// the cheats window over a running game.
    fn cheats(&mut self, ctx: &egui::Context, view: &View, enabled: bool) {
        let entries = self.cheat_entries(view);
        let directional = self.mode == InputMode::Directional;
        let footer = [(Nav::Accept, "Toggle"), (Nav::Back, "Back")];
        let mut add = None;
        let form = self.cheat_form;
        let mut cancel_form = false;
        let closed = window(ctx, "cheats", Vec2::new(620.0, 640.0), ("Cheats", Some("Turn codes on and off while you play")), enabled, Some((directional && !form, self.pad_hints, &footer)), |ui| {
            egui::ScrollArea::vertical().id_salt("cheats").auto_shrink(false).show(ui, |ui| {
                if form {
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Name").color(color::muted()));
                    ui.add(egui::TextEdit::singleline(&mut self.cheat_name).hint_text("Infinite money").desired_width(f32::INFINITY));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Code").color(color::muted()));
                    ui.add(egui::TextEdit::multiline(&mut self.cheat_code).hint_text("00000000 00000000").desired_rows(6).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if pill_button(ui, "Add cheat", true).clicked() && !self.cheat_code.trim().is_empty() {
                            add = Some((std::mem::take(&mut self.cheat_name), std::mem::take(&mut self.cheat_code)));
                        }
                        if pill_button(ui, "Cancel", false).clicked() {
                            cancel_form = true;
                        }
                    });
                } else {
                    self.draw_list(ui, &entries, ListOf::Cheats, ListStyle { row_height: 50.0, focused: true });
                }
            });
        });
        if let Some((name, code)) = add {
            self.cheat_form = false;
            self.pending_cheat = Some((name, code));
        }
        if cancel_form {
            self.cheat_form = false;
        }
        if closed {
            self.cheats_open = false;
            self.cheat_form = false;
        }
    }

    /// what was found in what was dropped or picked, to say where it goes.
    fn import_window(&mut self, ctx: &egui::Context, view: &View, enabled: bool) {
        let entries = self.import_entries(view);
        let directional = self.mode == InputMode::Directional;
        let footer = [(Nav::Accept, "Toggle"), (Nav::Back, "Cancel")];
        let closed = window(ctx, "import", Vec2::new(720.0, 680.0), ("Add to Zakuro", Some("Choose what to add and which games it is for")), enabled, Some((directional, self.pad_hints, &footer)), |ui| {
            egui::ScrollArea::vertical().id_salt("import").auto_shrink(false).show(ui, |ui| {
                self.draw_list(ui, &entries, ListOf::Import, ListStyle { row_height: 50.0, focused: true });
            });
        });
        if closed {
            self.import = None;
        }
    }

    /// a list that came up to pick from, over everything else; a click
    /// outside it closes it.
    fn popup(&mut self, ctx: &egui::Context) {
        let Some(popup) = self.select_popup.clone() else { return };
        let screen = ctx.content_rect();
        let width = 480.0f32.min(screen.width() - 24.0);
        // the title wraps, a long name, such as a texture pack's folder, with it
        let title = ctx.fonts_mut(|fonts| fonts.layout(popup.title.clone(), strong(18.0), color::text(), width - 48.0));
        let head = title.size().y + 38.0;
        let notes = choice_notes(popup.item);
        let row_height = if notes.is_empty() { 46.0 } else { 60.0 };
        let height = (popup.options.len() as f32 * (row_height + 2.0) + head + 36.0).min(screen.height() - 40.0);
        let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
        let mode = self.mode;
        let mut chosen = None;
        let mut closed = false;
        let mut hovered = None;
        pop_in(ctx, egui::LayerId::new(egui::Order::Foreground, egui::Id::new("select popup")), screen.center());
        let cover = screen.expand(screen.size().max_elem() * 0.1);
        egui::Area::new(egui::Id::new("select popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .movable(false)
            .show(ctx, |ui| {
                ui.set_clip_rect(cover);
                // the area covers the screen, so egui finds it under the pointer and
                // its lists take the mouse wheel
                ui.expand_to_include_rect(screen);
                let backdrop = ui.interact(screen, ui.id().with("backdrop"), Sense::click());
                ui.painter().rect_filled(cover, 0.0, color::scrim());
                ui.painter().rect_filled(rect, CornerRadius::same(16), color::surface());
                ui.painter().rect_stroke(rect, CornerRadius::same(16), Stroke::new(1.0, color::line()), StrokeKind::Inside);
                ui.painter().galley(rect.left_top() + Vec2::new(24.0, 20.0), title.clone(), color::text());
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(rect.min + Vec2::new(10.0, head), rect.max - Vec2::new(10.0, 12.0))));
                let time = child.ctx().input(|input| input.time);
                egui::ScrollArea::vertical().id_salt("popup").auto_shrink(false).show(&mut child, |ui| {
                    let highlight = Highlight::under(ui);
                    let mut target = None;
                    for (index, option) in popup.options.iter().enumerate() {
                        let (row_rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_height), Sense::click());
                        let response = response.on_hover_cursor(CursorIcon::PointingHand);
                        let selected = index == popup.selected;
                        if selected && mode == InputMode::Directional {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                        let mut row = Row::new(Item::Choice(index), option.clone(), if popup.current == Some(index) { Value::Check } else { Value::None });
                        row.note = notes.get(index).cloned();
                        if selected {
                            target = Some(row_rect);
                        }
                        paint_row(ui, row_rect, &row, selected, mode);
                        if response.hovered() && mode == InputMode::Pointer {
                            hovered = Some(index);
                        }
                        if response.clicked() {
                            chosen = Some(index);
                        }
                    }
                    highlight.paint(ui, target, mode, time);
                });
                if backdrop.clicked() && !ui.ctx().input(|input| input.pointer.interact_pos().is_some_and(|at| rect.contains(at))) {
                    closed = true;
                }
            });
        if let (Some(index), Some(popup)) = (hovered, &mut self.select_popup) {
            popup.selected = index;
        }
        if let Some(index) = chosen {
            self.clicked.push((Item::Choice(index), Op::Activate));
        } else if closed {
            self.select_popup = None;
        }
    }

    /// a message, until it is closed.
    fn message_window(&mut self, ctx: &egui::Context) {
        let Some(text) = self.message.clone() else { return };
        let screen = ctx.content_rect();
        let width = 520.0f32.min(screen.width() - 24.0);
        let mut close = false;
        let report = self.report.as_ref().filter(|(about, _)| *about == text).map(|(_, report)| report.clone());
        egui::Area::new(egui::Id::new("message"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .movable(false)
            .show(ctx, |ui| {
                ui.set_clip_rect(screen);
                ui.expand_to_include_rect(screen);
                let _ = ui.interact(screen, ui.id().with("backdrop"), Sense::click());
                ui.painter().rect_filled(screen, 0.0, color::scrim());
                let galley = ui.painter().layout(text.clone(), FontId::proportional(15.5), color::text(), width - 56.0);
                let height = (galley.size().y + 110.0).min(screen.height() - 40.0);
                let rect = Rect::from_center_size(screen.center(), Vec2::new(width, height));
                ui.painter().rect_filled(rect, CornerRadius::same(16), color::surface());
                ui.painter().rect_stroke(rect, CornerRadius::same(16), Stroke::new(1.0, color::line()), StrokeKind::Inside);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(28.0)));
                egui::ScrollArea::vertical().id_salt("message text").max_height(height - 110.0).show(&mut child, |ui| {
                    ui.label(egui::RichText::new(text.as_str()).size(15.5).color(color::text()));
                });
                child.add_space(14.0);
                child.horizontal(|ui| {
                    if let Some(report) = &report {
                        if pill_button(ui, "Copy info for a report", false).clicked() {
                            ui.ctx().copy_text(report.clone());
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if pill_button(ui, "OK", true).clicked() {
                            close = true;
                        }
                    });
                });
            });
        if close {
            self.message = None;
            self.report = None;
        }
    }

    /// the keyboard a game opened to have text typed in, which waits until
    /// a button closes it.
    fn keyboard(&mut self, ctx: &egui::Context, request: &Request) -> Option<Action> {
        let fresh = self.typing.as_ref().is_none_or(|(asked, _)| asked != request);
        if fresh {
            self.typing = Some((request.clone(), request.text.clone()));
        }
        let (_, text) = self.typing.as_mut()?;
        let confirm = request.buttons.len() - 1;
        let mut action = None;
        let screen = ctx.content_rect();
        egui::Area::new(egui::Id::new("game keyboard"))
            .order(egui::Order::Foreground)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                let _ = screen;
                egui::Frame::new().fill(color::surface()).corner_radius(CornerRadius::same(16)).inner_margin(egui::Margin::same(22)).stroke(Stroke::new(1.0, color::line())).show(ui, |ui| {
                    ui.label(egui::RichText::new("The game asks for text").size(16.0).color(color::muted()));
                    ui.add_space(8.0);
                    let field = ui.add(egui::TextEdit::singleline(text).hint_text(&request.hint).char_limit(request.max_length).desired_width(340.0).font(FontId::proportional(18.0)));
                    if fresh {
                        field.request_focus();
                    }
                    let valid = request.accepts(text);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        for (button, label) in request.buttons.iter().enumerate() {
                            let enabled = button != confirm || valid;
                            if ui.add_enabled_ui(enabled, |ui| pill_button(ui, label, button == confirm)).inner.clicked() {
                                action = Some(Action::Keyboard(text.clone(), button));
                            }
                        }
                    });
                    if valid && field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                        action = Some(Action::Keyboard(text.clone(), confirm));
                    }
                });
            });
        if action.is_some() {
            self.typing = None;
        }
        action
    }

    /// the multiplayer window: the room the player is in, or joining one
    /// and opening one. it is a form, with fields typed in, which the mouse
    /// and keyboard fill.
    fn multiplayer(&mut self, ctx: &egui::Context, settings: &mut Settings, room: Option<&RoomView>, enabled: bool) -> Vec<Action> {
        let mut actions = Vec::new();
        let password = &mut self.room_password;
        let dlp_icons = &mut self.dlp_icons;
        let closed = window(ctx, "multiplayer", Vec2::new(560.0, 600.0), ("Multiplayer", Some("Local wireless play goes through a room one of you opens")), enabled, None, |ui| {
            egui::ScrollArea::vertical().id_salt("multiplayer").auto_shrink(false).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.add_space(6.0);
                field_row(ui, "Nickname", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut settings.nickname).char_limit(10).hint_text("Player").desired_width(220.0));
                });
                ui.add_space(6.0);
                match room {
                    Some(room) => {
                        ui.label(egui::RichText::new(format!("In the room {}", room.name)).size(17.0).color(color::text()));
                        if let Some(port) = room.port {
                            ui.label(egui::RichText::new(format!("Open on port {port}. Others join with this computer's address and that port.")).color(color::muted()));
                        }
                        for (nickname, game) in &room.members {
                            let line = if game.is_empty() { nickname.clone() } else { format!("{nickname} · {game}") };
                            ui.label(egui::RichText::new(line).color(color::text()));
                        }
                        let leave = if room.port.is_some() { "Close the room" } else { "Leave the room" };
                        if pill_button(ui, leave, false).clicked() {
                            actions.push(Action::LeaveRoom);
                        }
                        if let Some(download_play) = &room.download_play {
                            ui.add_space(10.0);
                            ui.label(egui::RichText::new("Download Play").size(16.0).color(color::muted()));
                            match &download_play.doing {
                                Some((doing, progress)) => {
                                    ui.label(doing);
                                    if let Some(progress) = progress {
                                        ui.add(egui::ProgressBar::new(*progress).show_percentage());
                                    }
                                    if pill_button(ui, "Leave", false).clicked() {
                                        actions.push(Action::DlpLeave);
                                    }
                                }
                                None if download_play.offers.is_empty() => {
                                    ui.label(egui::RichText::new("No one in the room offers a game. A game offers one from its own Download Play menu.").color(color::muted()));
                                }
                                None => {
                                    for (index, offer) in download_play.offers.iter().enumerate() {
                                        ui.horizontal(|ui| {
                                            if let Some(pixels) = &offer.icon {
                                                let texture = dlp_icons.entry(offer.child).or_insert_with(|| {
                                                    let image = egui::ColorImage::from_rgba_unmultiplied([ICON_SIZE, ICON_SIZE], pixels);
                                                    ui.ctx().load_texture(format!("download play {:016X}", offer.child), image, egui::TextureOptions::LINEAR)
                                                });
                                                ui.add(egui::Image::new(&*texture).fit_to_exact_size(Vec2::splat(36.0)));
                                            }
                                            ui.vertical(|ui| {
                                                ui.label(egui::RichText::new(&offer.title).color(color::text()));
                                                ui.label(egui::RichText::new(format!("from {}, {} of {} joined", offer.host, offer.guests, offer.most)).color(color::muted()));
                                            });
                                            if ui.add_enabled_ui(offer.blocked.is_none(), |ui| pill_button(ui, "Join", true)).inner.clicked() {
                                                actions.push(Action::DlpJoin(index));
                                            }
                                        });
                                        if let Some(why) = &offer.blocked {
                                            ui.label(egui::RichText::new(why).color(color::muted()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    None => {
                        ui.label(egui::RichText::new("Join a room").size(16.0).color(color::muted()));
                        field_row(ui, "Address", |ui| {
                            ui.add(egui::TextEdit::singleline(&mut settings.room_address).hint_text("192.168.0.10:24873").desired_width(220.0));
                        });
                        field_row(ui, "Password", |ui| {
                            ui.add(egui::TextEdit::singleline(password).password(true).hint_text("none").desired_width(220.0));
                        });
                        if ui.add_enabled_ui(!settings.room_address.trim().is_empty(), |ui| pill_button(ui, "Join", true)).inner.clicked() {
                            actions.push(Action::JoinRoom);
                        }
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new("Open a room").size(16.0).color(color::muted()));
                        field_row(ui, "Name", |ui| {
                            ui.add(egui::TextEdit::singleline(&mut settings.room_name).hint_text("Zakuro").desired_width(220.0));
                        });
                        field_row(ui, "Port", |ui| {
                            ui.add(egui::DragValue::new(&mut settings.room_port).range(1024..=65535));
                        });
                        ui.label(egui::RichText::new("The password above, if any, is the room's.").color(color::faint()));
                        if pill_button(ui, "Open a room", true).clicked() {
                            actions.push(Action::HostRoom);
                        }
                    }
                }
            });
        });
        if closed {
            self.multiplayer_open = false;
        }
        actions
    }

    /// an install's progress, in a corner, with a way to stop it.
    fn install_progress(&mut self, ctx: &egui::Context, view: &View) {
        let doing = match (&view.install, view.identifying) {
            (Some(progress), _) => Some((progress.doing.clone(), Some(progress.fraction()))),
            (None, true) => Some(("Looking at what was added…".to_owned(), None)),
            _ => None,
        };
        let toast = self.toast.as_ref().filter(|(_, since)| since.elapsed() < std::time::Duration::from_millis(2600)).map(|(text, _)| text.clone());
        if toast.is_none() {
            self.toast = None;
        }
        let mut cancel = false;
        if doing.is_some() || toast.is_some() {
            egui::Area::new(egui::Id::new("status"))
                .order(egui::Order::Foreground)
                .anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -28.0))
                .show(ctx, |ui| {
                    egui::Frame::new().fill(color::raised()).corner_radius(CornerRadius::same(22)).inner_margin(egui::Margin::symmetric(20, 12)).show(ui, |ui| {
                        if let Some((doing, fraction)) = &doing {
                            ui.horizontal(|ui| {
                                match fraction {
                                    Some(fraction) => {
                                        ui.add(egui::ProgressBar::new(*fraction).desired_width(180.0).corner_radius(CornerRadius::same(4)).fill(color::accent()));
                                    }
                                    None => {
                                        ui.spinner();
                                    }
                                }
                                ui.label(egui::RichText::new(truncate(doing, 320.0, 14.0)).size(14.0).color(color::text()));
                                if view.install.is_some() && pill_button(ui, "Stop", false).clicked() {
                                    cancel = true;
                                }
                            });
                        } else if let Some(text) = &toast {
                            ui.label(egui::RichText::new(text).size(14.5).color(color::text()));
                        }
                    });
                });
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if cancel {
            self.pending_cancel_install = true;
        }
    }
}

/// a label and its field, side by side.
fn field_row(ui: &mut egui::Ui, label: &str, field: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(100.0, 24.0), Sense::hover());
        ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(15.0), color::muted());
        field(ui);
    });
}

/// a rounded button, filled when it is the main one.
fn pill_button(ui: &mut egui::Ui, text: &str, primary: bool) -> egui::Response {
    let (fill, text_color) = if primary { (color::accent(), color::bg()) } else { (color::raised(), color::text()) };
    let enabled = ui.is_enabled();
    let button = egui::Button::new(egui::RichText::new(text).size(15.0).color(if enabled { text_color } else { color::faint() }))
        .fill(if enabled { fill } else { color::surface() })
        .corner_radius(CornerRadius::same(18))
        .min_size(Vec2::new(96.0, 38.0));
    let response = ui.add(button);
    if enabled { response.on_hover_cursor(CursorIcon::PointingHand) } else { response }
}

// Home

/// the picture behind Home, decoded away from the interface.
#[derive(Default)]
struct Background {
    /// the file shown or being read.
    path: Option<PathBuf>,
    texture: Option<egui::TextureHandle>,
    loading: Option<Receiver<Result<egui::ColorImage, String>>>,
}

/// the longest side a background is kept at, bigger ones are scaled down.
const BACKGROUND_SIDE: u32 = 2560;

impl Menus {
    /// lets go of the pictures uploaded to the interface, which a new one,
    /// with another presenter, has to upload again.
    pub fn forget_textures(&mut self) {
        self.icons.clear();
        self.dlp_icons.clear();
        self.background = Background::default();
    }
}

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
fn cover(area: Vec2, picture: [usize; 2]) -> Rect {
    let (width, height) = (picture[0].max(1) as f32, picture[1].max(1) as f32);
    let area_ratio = area.x / area.y.max(1.0);
    let picture_ratio = width / height;
    if area_ratio > picture_ratio {
        // wider area, the picture's top and bottom are cut
        let shown = picture_ratio / area_ratio;
        Rect::from_min_max(Pos2::new(0.0, (1.0 - shown) / 2.0), Pos2::new(1.0, (1.0 + shown) / 2.0))
    } else {
        let shown = area_ratio / picture_ratio;
        Rect::from_min_max(Pos2::new((1.0 - shown) / 2.0, 0.0), Pos2::new((1.0 + shown) / 2.0, 1.0))
    }
}

/// the average color of an icon, for the tile behind it.
fn average(pixels: &[u8]) -> Color32 {
    let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
    for pixel in pixels.as_chunks::<4>().0.iter().filter(|pixel| pixel[3] > 128) {
        r += pixel[0] as u64;
        g += pixel[1] as u64;
        b += pixel[2] as u64;
        n += 1;
    }
    if n == 0 {
        return color::raised();
    }
    Color32::from_rgb((r / n) as u8, (g / n) as u8, (b / n) as u8)
}

/// how long Home takes to fade out over a game starting, in seconds.
const HOME_FADE: f64 = 0.45;

/// how much bigger the selected tile grows, of its side.
const TILE_GROW: f32 = 0.07;

impl Menus {
    /// Home's color fading out over a game that just started, rather than
    /// the screen cutting from one to the other.
    fn fade_from_home(&mut self, ctx: &egui::Context, running: bool) {
        if !running {
            self.game_shown_at = None;
            return;
        }
        let now = ctx.input(|input| input.time);
        let shown = *self.game_shown_at.get_or_insert(now);
        let t = ((now - shown) / HOME_FADE).clamp(0.0, 1.0) as f32;
        if t >= 1.0 {
            return;
        }
        let alpha = 1.0 - egui::emath::easing::cubic_out(t);
        let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new("home fade"));
        ctx.layer_painter(layer).rect_filled(ctx.content_rect(), 0.0, color::bg().gamma_multiply(alpha));
        ctx.request_repaint();
    }

    /// the Home screen: the games in a row, the selected one's name above
    /// it, and round buttons under it.
    fn home(&mut self, ui: &mut egui::Ui, settings: &Settings, view: &View) {
        let library = view.library;
        let screen = ui.max_rect();
        let painter = ui.painter().clone();
        painter.rect_filled(screen, 0.0, color::bg());
        match self.background.get(ui.ctx(), settings.background.as_deref()) {
            Ok(Some(texture)) => {
                let alpha = (settings.background_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
                painter.image(texture.id(), screen, cover(screen.size(), texture.size()), Color32::from_white_alpha(alpha));
            }
            Ok(None) => {}
            Err(error) => self.message = Some(error),
        }
        let time = ui.ctx().input(|input| input.time);
        let directional = self.mode == InputMode::Directional;

        // the top bar
        painter.text(screen.left_top() + Vec2::new(36.0, 30.0), Align2::LEFT_CENTER, "Zakuro", strong(18.0), color::muted());
        let status = if library.is_scanning() {
            "Looking for games…".to_owned()
        } else {
            let count = library.games.iter().filter(|game| game.problem.is_none()).count();
            if count == 1 { "1 game".to_owned() } else { format!("{count} games") }
        };
        painter.text(screen.right_top() + Vec2::new(-36.0, 30.0), Align2::RIGHT_CENTER, status, FontId::proportional(15.0), color::faint());
        self.paint_recompile_card(&painter, screen, view);

        // the tiles
        let tiles = library.games.len() + 1;
        self.selected_game = self.selected_game.min(tiles - 1);
        let side = (screen.height() * 0.30).clamp(150.0, 260.0).min(screen.width() * 0.4);
        let gap = side * 0.09;
        let row_top = screen.top() + (screen.height() * 0.38 - side / 2.0).max(118.0);
        let left_margin = 64.0;
        let visible = ((screen.width() - left_margin * 2.0 + gap) / (side + gap)).floor().max(1.0);
        // the row scrolls to keep the selection in sight
        let target = {
            let selected = self.selected_game as f32;
            let mut scroll = self.home_scroll;
            if selected < scroll {
                scroll = selected;
            }
            if selected > scroll + visible - 1.0 {
                scroll = selected - visible + 1.0;
            }
            scroll.clamp(0.0, (tiles as f32 - visible).max(0.0))
        };
        self.home_scroll = target;
        let scroll = ui.ctx().animate_value_with_time(egui::Id::new("home scroll"), target, 0.18);
        let mut hovered_tile = None;
        let mut clicked_tile = None;
        let mut options_tile = None;
        // the selected tile is drawn last, so it grows over its neighbours
        let mut drawn = Vec::new();
        for index in 0..tiles {
            let x = screen.left() + left_margin + (index as f32 - scroll) * (side + gap);
            let rect = Rect::from_min_size(Pos2::new(x, row_top), Vec2::splat(side));
            if rect.right() < screen.left() - side || rect.left() > screen.right() + side {
                continue;
            }
            let response = ui.interact(rect, egui::Id::new(("tile", index)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let selected = index == self.selected_game && self.home_focus == HomeFocus::Games;
            if response.hovered() && self.mode == InputMode::Pointer && ui.is_enabled() {
                hovered_tile = Some(index);
            }
            if response.clicked() {
                clicked_tile = Some(index);
            }
            if response.secondary_clicked() {
                options_tile = Some(index);
            }
            // tiles grow when selected and shrink back after, in a moment
            let grow = ui.ctx().animate_bool_with_time(egui::Id::new(("tile grow", index)), selected, 0.14);
            drawn.push((selected, index, rect.expand(side * TILE_GROW / 2.0 * egui::emath::easing::cubic_out(grow))));
        }
        drawn.sort_by_key(|(selected, _, _)| *selected);
        for (selected, index, rect) in drawn {
            match library.games.get(index) {
                Some(game) => self.paint_tile(ui, rect, game, selected, time, view),
                None => paint_add_tile(&painter, rect, selected, directional, time),
            }
        }
        if let Some(index) = hovered_tile {
            self.selected_game = index;
            self.home_focus = HomeFocus::Games;
        }

        // the selected game's name and what it has, above and below
        let selected_x = screen.left() + left_margin + (self.selected_game as f32 - scroll) * (side + gap);
        let (title, details, problem) = match library.games.get(self.selected_game) {
            Some(game) => (game.name.clone(), game_details(game, settings, view), game.problem.clone()),
            None if library.games.is_empty() => (
                "Add your games".to_owned(),
                "Choose .3ds and .cia files, or drag them onto this window. Updates, DLC, texture packs and cheats are recognized too.".to_owned(),
                None,
            ),
            None => ("Add games".to_owned(), "Games, updates, DLC, texture packs and cheat files".to_owned(), None),
        };
        let title_x = selected_x.clamp(screen.left() + 36.0, (screen.right() - 420.0).max(screen.left() + 36.0));
        // clear of the selected tile once it has grown
        let grown = side * TILE_GROW / 2.0;
        if self.home_focus == HomeFocus::Games {
            painter.text(Pos2::new(title_x, row_top - 22.0 - grown), Align2::LEFT_BOTTOM, &title, strong(26.0), color::text());
        }
        let details_galley = painter.layout(details, FontId::proportional(14.5), color::muted(), (screen.width() - 120.0).min(760.0));
        painter.galley(Pos2::new(title_x, row_top + side + 22.0 + grown), details_galley, color::muted());
        if let Some(problem) = problem {
            painter.text(Pos2::new(title_x, row_top + side + 48.0 + grown), Align2::LEFT_TOP, problem, FontId::proportional(14.5), color::warn());
        }

        // the round buttons
        // close under the games, as a console's are, never off the screen
        let dock_y = (row_top + side + 130.0).min(screen.bottom() - 90.0);
        let dock_size = 60.0;
        let dock_gap = 28.0;
        let dock_width = DOCK.len() as f32 * dock_size + (DOCK.len() - 1) as f32 * dock_gap;
        let mut hovered_dock = None;
        for (index, button) in DOCK.iter().enumerate() {
            let center = Pos2::new(screen.center().x - dock_width / 2.0 + dock_size / 2.0 + index as f32 * (dock_size + dock_gap), dock_y);
            let rect = Rect::from_center_size(center, Vec2::splat(dock_size));
            let response = ui.interact(rect, egui::Id::new(("dock", index)), Sense::click()).on_hover_cursor(CursorIcon::PointingHand);
            let selected = self.home_focus == HomeFocus::Dock && self.dock == index;
            let hovered = response.hovered() && self.mode == InputMode::Pointer && ui.is_enabled();
            if hovered {
                hovered_dock = Some(index);
            }
            painter.circle_filled(center, dock_size / 2.0, if selected || hovered { color::hover() } else { color::surface() });
            if selected && directional {
                let glow = (0.75 + 0.25 * (time * 3.2).sin()) as f32;
                painter.circle_stroke(center, dock_size / 2.0 + 3.0, Stroke::new(3.0, color::accent().gamma_multiply(glow)));
            }
            paint_dock_icon(&painter, center, *button);
            if selected || hovered {
                painter.text(center + Vec2::new(0.0, dock_size / 2.0 + 16.0), Align2::CENTER_CENTER, button.name(), FontId::proportional(13.5), color::text());
            }
            if response.clicked() {
                self.clicked_dock = Some(*button);
            }
        }
        if let Some(index) = hovered_dock {
            self.dock = index;
            self.home_focus = HomeFocus::Dock;
        }

        // the button hints
        if directional && ui.is_enabled() {
            let foot = Rect::from_min_max(Pos2::new(screen.left(), screen.bottom() - 48.0), screen.max);
            let list: Vec<(Nav, &str)> = match (self.home_focus, library.games.get(self.selected_game)) {
                (HomeFocus::Games, Some(_)) if crate::recompile::blocking_play(view.jobs, view.waiting).is_some() => {
                    vec![(Nav::Y, "Options"), (Nav::Start, "Settings")]
                }
                (HomeFocus::Games, Some(game)) if game.problem.is_none() => vec![(Nav::Accept, "Start"), (Nav::Y, "Options"), (Nav::Start, "Settings")],
                (HomeFocus::Games, _) | (HomeFocus::Dock, _) => vec![(Nav::Accept, "OK"), (Nav::Start, "Settings")],
            };
            hints(ui, foot, self.pad_hints, &list);
        }

        if let Some(index) = clicked_tile {
            self.selected_game = index;
            self.home_focus = HomeFocus::Games;
            self.clicked_tile = Some(index);
        }
        if let Some(index) = options_tile {
            if let Some(game) = library.games.get(index).filter(|game| game.problem.is_none()) {
                self.open_game_settings(game.program_id, game.name.clone(), view.data_dir);
            }
        }
    }

    /// the recompile going on, at the top of Home: the game, its stage, how
    /// far along, about how long is left, and how many wait their turn.
    fn paint_recompile_card(&self, painter: &egui::Painter, screen: Rect, view: &View) {
        let Some(job) = view.jobs.iter().find(|job| !job.stage().finished()) else { return };
        let busy = Busy::Recompiling { stage: job.stage(), compiling: job.compiling_for() };
        let width = 460.0f32.min(screen.width() - 300.0).max(240.0);
        let card = Rect::from_center_size(Pos2::new(screen.center().x, 44.0 + screen.top()), Vec2::new(width, 64.0));
        painter.rect_filled(card, CornerRadius::same(14), color::surface());
        painter.rect_stroke(card, CornerRadius::same(14), Stroke::new(1.0, color::line()), StrokeKind::Inside);
        let inner = card.shrink2(Vec2::new(16.0, 10.0));
        let waiting = view.waiting.len();
        let title = if waiting > 0 { format!("Recompiling {}  (+{waiting} waiting)", job.name) } else { format!("Recompiling {}", job.name) };
        painter.text(inner.left_top(), Align2::LEFT_TOP, truncate(&title, inner.width(), 15.0), strong(15.0), color::text());
        painter.text(inner.left_top() + Vec2::new(0.0, 21.0), Align2::LEFT_TOP, busy.describe(), FontId::proportional(13.0), color::muted());
        let bar = Rect::from_min_size(Pos2::new(inner.left(), inner.bottom() - 5.0), Vec2::new(inner.width(), 5.0));
        painter.rect_filled(bar, CornerRadius::same(3), color::raised());
        let done = Rect::from_min_size(bar.min, Vec2::new(bar.width() * busy.fraction(), bar.height()));
        painter.rect_filled(done, CornerRadius::same(3), color::accent());
    }

    /// a game's tile: its icon over its colors, the outline when selected,
    /// and a recompile's progress along its foot.
    fn paint_tile(&mut self, ui: &egui::Ui, rect: Rect, game: &Game, selected: bool, time: f64, view: &View) {
        let painter = ui.painter();
        let radius = CornerRadius::same(18);
        let icon = game.icon.as_ref().map(|pixels| {
            let texture = self.icons.entry(game.path.clone()).or_insert_with(|| {
                let image = egui::ColorImage::from_rgba_unmultiplied([ICON_SIZE, ICON_SIZE], pixels);
                ui.ctx().load_texture(game.path.display().to_string(), image, egui::TextureOptions::NEAREST)
            });
            (texture.id(), average(pixels))
        });
        let tint = icon.map_or(color::raised(), |(_, average)| average);
        // the icon's own colour, toned toward the background
        if color::is_light() {
            painter.rect_filled(rect, radius, tint.lerp_to_gamma(Color32::WHITE, 0.72));
        } else {
            painter.rect_filled(rect, radius, tint.gamma_multiply(0.55));
            painter.rect_filled(rect, radius, Color32::from_black_alpha(70));
        }
        match icon {
            Some((texture, _)) => {
                // the icon at a whole number of times its size, sharp
                let scale = ((rect.width() * 0.62) / ICON_SIZE as f32).floor().max(1.0);
                let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(ICON_SIZE as f32 * scale));
                painter.image(texture, icon_rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                let name = painter.layout(game.name.clone(), FontId::proportional(16.0), color::text(), rect.width() - 28.0);
                painter.galley(rect.center() - galley_half(&name), name, color::text());
            }
        }
        if game.problem.is_some() {
            painter.rect_filled(rect, radius, Color32::from_black_alpha(140));
            painter.text(rect.center(), Align2::CENTER_CENTER, "!", FontId::proportional(42.0), color::warn());
        }
        // a game recompiling is greyed out, as it can't be played until then
        if let Some(busy) = Busy::of(game.program_id, view.jobs, view.waiting) {
            painter.rect_filled(rect, radius, Color32::from_black_alpha(190));
            let label = match &busy {
                Busy::Waiting => "Waiting".to_owned(),
                Busy::Recompiling { .. } => format!("{:.0}%", busy.fraction() * 100.0),
            };
            painter.text(rect.center() - Vec2::new(0.0, 8.0), Align2::CENTER_CENTER, label, strong(30.0), Color32::WHITE);
            painter.text(rect.center() + Vec2::new(0.0, 22.0), Align2::CENTER_CENTER, "Recompiling", FontId::proportional(14.0), Color32::from_gray(200));
            let bar = Rect::from_min_size(Pos2::new(rect.left() + 14.0, rect.bottom() - 18.0), Vec2::new(rect.width() - 28.0, 6.0));
            painter.rect_filled(bar, CornerRadius::same(3), Color32::from_black_alpha(160));
            let done = Rect::from_min_size(bar.min, Vec2::new(bar.width() * busy.fraction(), bar.height()));
            painter.rect_filled(done, CornerRadius::same(3), color::accent());
        }
        if selected {
            let glow = if self.mode == InputMode::Directional { (0.8 + 0.2 * (time * 3.2).sin()) as f32 } else { 1.0 };
            painter.rect_stroke(rect.expand(5.0), CornerRadius::same(22), Stroke::new(4.0, color::accent().gamma_multiply(glow)), StrokeKind::Inside);
        }
    }
}

fn galley_half(galley: &egui::Galley) -> Vec2 {
    galley.size() / 2.0
}

/// the tile that adds games, last in the row.
fn paint_add_tile(painter: &egui::Painter, rect: Rect, selected: bool, directional: bool, time: f64) {
    painter.rect_filled(rect, CornerRadius::same(18), color::surface());
    painter.rect_stroke(rect.shrink(1.0), CornerRadius::same(18), Stroke::new(1.5, color::line()), StrokeKind::Inside);
    let center = rect.center() - Vec2::new(0.0, 14.0);
    let arm = rect.width() * 0.11;
    crate::icons::paint(painter, Rect::from_center_size(center, Vec2::splat(arm * 2.6)), Icon::Plus, color::muted());
    painter.text(rect.center() + Vec2::new(0.0, arm + 18.0), Align2::CENTER_CENTER, "Add games", FontId::proportional(15.0), color::muted());
    if selected {
        let glow = if directional { (0.8 + 0.2 * (time * 3.2).sin()) as f32 } else { 1.0 };
        painter.rect_stroke(rect.expand(5.0), CornerRadius::same(22), Stroke::new(4.0, color::accent().gamma_multiply(glow)), StrokeKind::Inside);
    }
}

/// a round button's icon.
fn paint_dock_icon(painter: &egui::Painter, center: Pos2, button: Dock) {
    let icon = match button {
        Dock::Add => Icon::Plus,
        Dock::Multiplayer => Icon::Users,
        Dock::Settings => Icon::Sliders,
        Dock::Quit => Icon::Power,
    };
    crate::icons::paint(painter, Rect::from_center_size(center, Vec2::splat(26.0)), icon, color::text());
}

/// what a game has, in a line: its update, DLC, textures, recompiled code.
fn game_details(game: &Game, settings: &Settings, view: &View) -> String {
    let mut parts = Vec::new();
    if !game.publisher.is_empty() {
        parts.push(game.publisher.clone());
    }
    if let Some(update) = &game.update {
        parts.push(format!("Update {}", zakuro_fs::version_name(update.version)));
    }
    if !game.dlc.is_empty() {
        parts.push("DLC".to_owned());
    }
    if game.textured {
        parts.push("Custom textures".to_owned());
    }
    if game.outdated {
        parts.push("Recompiled (outdated)".to_owned());
    } else if game.recompiled {
        parts.push("Recompiled".to_owned());
    }
    if settings.has_own(game.program_id) {
        parts.push("Custom options".to_owned());
    }
    if let Some(busy) = Busy::of(game.program_id, view.jobs, view.waiting) {
        parts.push(match busy {
            Busy::Waiting => "Waiting to recompile".to_owned(),
            Busy::Recompiling { .. } => format!("Recompiling {:.0}%, playable once it finishes", busy.fraction() * 100.0),
        });
    }
    parts.join("  ·  ")
}

impl Menus {
    /// what goes over a running game: the frame rate and fast forward, and
    /// the menu when it is open.
    fn over(&mut self, ui: &mut egui::Ui, running: &RunningView, view: &View) {
        let ctx = ui.ctx().clone();
        if running.fast {
            badge(&ctx, "fast forward", Align2::RIGHT_TOP, "» Fast forward");
        }
        if let Some(fps) = running.fps {
            badge(&ctx, "fps", Align2::LEFT_TOP, &format!("{fps:.0} fps"));
        }
        if !self.menu_open {
            return;
        }
        let entries = self.pause_entries(running);
        let screen = ctx.content_rect();
        let directional = self.mode == InputMode::Directional;
        let height = (rows(&entries).count() as f32 * 46.0 + 104.0 + if directional { 48.0 } else { 0.0 }).min(screen.height() - 24.0);
        let rect = Rect::from_center_size(screen.center(), Vec2::new(400.0f32.min(screen.width() - 24.0), height));
        let jobs: Vec<&Job> = view.jobs.iter().filter(|job| !job.stage().finished()).collect();
        pop_in(&ctx, egui::LayerId::new(egui::Order::Middle, egui::Id::new("pause menu")), screen.center());
        let cover = screen.expand(screen.size().max_elem() * 0.1);
        egui::Area::new(egui::Id::new("pause menu"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .movable(false)
            .show(&ctx, |ui| {
                ui.set_clip_rect(cover);
                // the area covers the screen, so egui finds it under the pointer and
                // its lists take the mouse wheel
                ui.expand_to_include_rect(screen);
                let _ = ui.interact(screen, ui.id().with("backdrop"), Sense::click());
                ui.painter().rect_filled(cover, 0.0, color::scrim());
                ui.painter().rect_filled(rect, CornerRadius::same(18), color::panel());
                ui.painter().rect_stroke(rect, CornerRadius::same(18), Stroke::new(1.0, color::line()), StrokeKind::Inside);
                ui.painter().text(rect.left_top() + Vec2::new(26.0, 24.0), Align2::LEFT_TOP, truncate(running.name, 330.0, 20.0), strong(20.0), color::text());
                let how = if running.recompiled { "Recompiled code" } else { "Interpreted" };
                let line = match jobs.first() {
                    Some(job) => format!("{how}  ·  {} {}", job.name, stage_name(&job.stage())),
                    None => how.to_owned(),
                };
                ui.painter().text(rect.left_top() + Vec2::new(26.0, 52.0), Align2::LEFT_TOP, line, FontId::proportional(13.0), color::muted());
                let foot = if directional { 44.0 } else { 0.0 };
                let body = Rect::from_min_max(rect.min + Vec2::new(10.0, 82.0), rect.max - Vec2::new(10.0, 10.0 + foot));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
                egui::ScrollArea::vertical().id_salt("pause").auto_shrink(false).show(&mut child, |ui| {
                    self.draw_list(ui, &entries, ListOf::Pause, ListStyle { row_height: 46.0, focused: true });
                });
                if directional {
                    let foot = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - foot - 4.0), rect.max);
                    hints(ui, foot, self.pad_hints, &[(Nav::Accept, "Select"), (Nav::Back, "Resume")]);
                }
            });
    }
}

/// a small label in a corner of the game.
fn badge(ctx: &egui::Context, id: &str, corner: Align2, text: &str) {
    let offset = Vec2::new(if corner.x() == egui::Align::Max { -10.0 } else { 10.0 }, 10.0);
    egui::Area::new(egui::Id::new(id)).anchor(corner, offset).interactable(false).show(ctx, |ui| {
        egui::Frame::new().fill(Color32::from_black_alpha(170)).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
            ui.add(egui::Label::new(egui::RichText::new(text).size(13.5).monospace().color(Color32::WHITE)).extend());
        });
    });
}

/// a line a GameOverlay keeps over the game, in the bottom right corner.
pub fn corner(ctx: &egui::Context, text: &str) {
    egui::Area::new(egui::Id::new("overlay corner")).anchor(Align2::RIGHT_BOTTOM, Vec2::new(-8.0, -8.0)).interactable(false).show(ctx, |ui| {
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

    fn view(library: &Library) -> View<'_> {
        View {
            library,
            jobs: &[],
            waiting: &[],
            running: None,
            cheats: &[],
            install: None,
            identifying: false,
            room: None,
            keyboard: None,
            corner: None,
            data_dir: None,
        }
    }

    /// runs frames of the interface on a screen of this size.
    fn frames(menus: &mut Menus, settings: &mut Settings, library: &Library, size: Vec2, count: usize) -> Vec<Action> {
        let ctx = crate::gui::test_context();
        let mut actions = Vec::new();
        for _ in 0..count {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..Default::default() };
            ctx.run_ui(input, |ui| actions.extend(menus.show(ui, settings, &view(library)))).drop_without_applying_deltas();
        }
        actions
    }

    #[test]
    fn a_background_fills_the_area_without_stretching() {
        let wide = cover(Vec2::new(200.0, 100.0), [100, 100]);
        assert_eq!((wide.min.x, wide.max.x), (0.0, 1.0));
        assert_eq!((wide.min.y, wide.max.y), (0.25, 0.75));
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
        assert!(decode(Path::new("/nowhere/at/all.png")).is_err());
    }

    #[test]
    fn every_screen_draws_on_a_small_window() {
        let library = Library::default();
        let mut settings = Settings::default();
        let small = Vec2::new(640.0, 360.0);
        let mut menus = Menus::default();
        frames(&mut menus, &mut settings, &library, small, 2);
        menus.open_settings();
        for section in 0..Section::SYSTEM.len() {
            menus.page.section = section;
            frames(&mut menus, &mut settings, &library, small, 2);
        }
        menus.open_game_settings(0x0004_0000_0012_3400, "A game".into(), None);
        for section in 0..Section::GAME.len() {
            menus.page.section = section;
            frames(&mut menus, &mut settings, &library, small, 2);
        }
        assert!(settings.per_game.is_empty(), "opening a game's options gives it nothing of its own");
        menus.message = Some("Something went wrong.\n\n".repeat(200));
        frames(&mut menus, &mut settings, &library, small, 2);
    }

    #[test]
    fn the_import_window_shows_and_adds_what_was_chosen() {
        let library = Library::default();
        let mut settings = Settings::default();
        let mut menus = Menus::default();
        let found = vec![
            Found::Title { path: "D:/a.3ds".into(), title_id: 1, kind: zakuro_fs::TitleKind::Game, version: 0, name: "A".into(), size: 10 },
            Found::Textures { path: "D:/HD".into(), name: "HD".into(), zip: false, images: 3, size: 30, title_id: None },
            Found::Unusable { path: "D:/notes.txt".into(), why: "No cheats in it".into() },
        ];
        menus.open_import(found, &library, None);
        frames(&mut menus, &mut settings, &library, Vec2::new(800.0, 480.0), 2);
        let v = view(&library);
        // the game is ticked; the pack has no game to go to, so it stays out
        let rows: Vec<Item> = rows(&menus.import_entries(&v)).map(|row| row.item).collect();
        let add = rows.iter().position(|item| *item == Item::ImportAdd).unwrap();
        menus.import_row = add;
        let actions = menus.navigate(Nav::Accept, &mut settings, &v);
        let [Action::Install(choice)] = &actions[..] else { panic!("{actions:?}") };
        assert_eq!(choice.found.len(), 1, "{choice:?}");
        assert!(menus.import.is_none());
    }

    #[test]
    fn the_menu_over_a_game_and_its_cheats_draw() {
        let library = Library::default();
        let mut settings = Settings::default();
        let mut menus = Menus { menu_open: true, cheats_open: false, slot: 1, ..Menus::default() };
        let cheats = [CheatView { name: "No Outlines".into(), enabled: false, notes: "Removes outlines".into(), builtin: true }];
        let ctx = crate::gui::test_context();
        for open_cheats in [false, true] {
            menus.cheats_open = open_cheats;
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 480.0))), ..Default::default() };
            let running = RunningView { name: "A game", program_id: Some(7), recompiled: false, saved: None, fps: Some(60.0), fast: false, started: None };
            let v = View { running: Some(running), cheats: &cheats, ..view(&library) };
            ctx.run_ui(input, |ui| drop(menus.show(ui, &mut settings, &v))).drop_without_applying_deltas();
        }
        // A on the enhancement turns it on
        let running = RunningView { name: "A game", program_id: Some(7), recompiled: false, saved: None, fps: None, fast: false, started: None };
        let v = View { running: Some(running), cheats: &cheats, ..view(&library) };
        assert_eq!(menus.navigate(Nav::Accept, &mut settings, &v), [Action::Cheat(0, true)]);
        // back closes the cheats, then resumes
        menus.navigate(Nav::Back, &mut settings, &v);
        assert!(!menus.cheats_open);
        assert_eq!(menus.navigate(Nav::Back, &mut settings, &v), [Action::Resume]);
    }

    #[test]
    fn recompiling_is_offered_on_home_only() {
        let library = Library::default();
        let mut settings = Settings::default();
        let mut menus = Menus { game_settings: Some((7, "A game".into())), ..Menus::default() };
        menus.page.section = Section::GAME.iter().position(|&section| section == Section::System).unwrap();
        let has_recompile = |menus: &Menus, v: &View| {
            menus.page_entries(&Settings::default(), v).iter().any(|entry| matches!(entry, Entry::Row(row) if row.item == Item::Recompile))
        };
        assert!(has_recompile(&menus, &view(&library)));
        let running = RunningView { name: "A game", program_id: Some(7), recompiled: false, saved: None, fps: None, fast: false, started: None };
        let v = View { running: Some(running), ..view(&library) };
        assert!(!has_recompile(&menus, &v));
        assert!(menus.handle(Item::Recompile, Op::Activate, &mut settings, &v).is_empty());
        assert!(menus.ask.is_none());
    }

    #[test]
    fn a_games_change_is_its_own_and_matching_the_system_drops_it() {
        let mut settings = Settings::default();
        let game = Scope::Game(7);
        choose(Item::Filter, Filter::ALL.iter().position(|&f| f == Filter::SmoothEdges).unwrap(), &mut settings, game);
        assert_eq!(settings.for_game(Some(7)).filter, Filter::SmoothEdges);
        assert_eq!(settings.filter, Filter::Sharp, "the system keeps its own");
        assert!(owned(&settings, game, Item::Filter));
        assert!(setting(Item::Filter, &settings, game).changed);
        // picking the system's value again leaves the game nothing of its own
        choose(Item::Filter, Filter::ALL.iter().position(|&f| f == Filter::Sharp).unwrap(), &mut settings, game);
        assert!(!settings.has_own(7));
        assert!(settings.per_game.is_empty());
    }

    #[test]
    fn x_puts_a_games_setting_back() {
        let mut settings = Settings::default();
        let library = Library::default();
        let mut menus = Menus::default();
        menus.open_game_settings(7, "A game".into(), None);
        toggle(Item::IntegerScale, &mut settings, Scope::Game(7));
        assert!(settings.has_own(7));
        menus.handle(Item::IntegerScale, Op::Reset, &mut settings, &view(&library));
        assert!(!settings.has_own(7));
    }

    #[test]
    fn the_same_press_moves_any_list() {
        let mut settings = Settings::default();
        let library = Library::default();
        let mut menus = Menus::default();
        menus.open_settings();
        let v = view(&library);
        // into the list and choose its first row: the resolution's list comes up
        menus.navigate(Nav::Accept, &mut settings, &v);
        menus.navigate(Nav::Accept, &mut settings, &v);
        let popup = menus.select_popup.clone().expect("a list to pick from");
        assert_eq!(popup.item, Item::Resolution);
        menus.navigate(Nav::Down, &mut settings, &v);
        menus.navigate(Nav::Accept, &mut settings, &v);
        assert_eq!(settings.resolution, 4);
        assert!(menus.select_popup.is_none());
        // right steps it on, without a list
        menus.navigate(Nav::Right, &mut settings, &v);
        assert_eq!(settings.resolution, 5);
        // back leaves the list, then the settings
        menus.navigate(Nav::Back, &mut settings, &v);
        assert!(menus.settings_open);
        menus.navigate(Nav::Back, &mut settings, &v);
        assert!(!menus.settings_open);
    }

    #[test]
    fn back_closes_what_is_on_top_first() {
        let mut menus = Menus::default();
        menus.open_settings();
        menus.select_popup = Some(SelectPopup { item: Item::Filter, scope: Scope::System, title: "Filter".into(), options: vec!["a".into()], selected: 0, current: None });
        menus.close_modals();
        assert!(menus.settings_open, "the list goes before the settings");
        menus.close_modals();
        assert!(!menus.modal_open());
    }

    #[test]
    fn a_game_recompiling_or_waiting_is_busy_with_its_progress() {
        let jobs = [Job::at_stage(1, Stage::Compiling { done: 50, total: 100 }), Job::at_stage(2, Stage::Done)];
        let waiting = [Wanted { program_id: 3, path: PathBuf::from("c.3ds") }];
        assert!(matches!(Busy::of(1, &jobs, &waiting), Some(Busy::Recompiling { .. })));
        assert_eq!(Busy::of(2, &jobs, &waiting), None);
        assert_eq!(Busy::of(3, &jobs, &waiting), Some(Busy::Waiting));
        assert_eq!(Busy::of(4, &jobs, &waiting), None);
        let halfway = Busy::Recompiling { stage: Stage::Compiling { done: 50, total: 100 }, compiling: Some(std::time::Duration::from_secs(300)) };
        assert_eq!(halfway.describe(), "Compiling 50/100  ·  50%  ·  about 5 minutes left");
        let early = Busy::Recompiling { stage: Stage::Compiling { done: 2, total: 100 }, compiling: Some(std::time::Duration::from_secs(5)) };
        assert!(!early.describe().contains("left"));
        assert_eq!(duration_name(30.0), "under a minute");
        assert_eq!(duration_name(4000.0), "1h 06m");
    }

    #[test]
    fn home_moves_between_the_games_and_the_buttons() {
        let mut settings = Settings::default();
        let library = Library::default();
        let mut menus = Menus::default();
        let v = view(&library);
        // no games, the add tile is there to choose
        assert_eq!(menus.navigate(Nav::Accept, &mut settings, &v), [Action::AddContent(None)]);
        menus.navigate(Nav::Down, &mut settings, &v);
        assert_eq!(menus.home_focus, HomeFocus::Dock);
        menus.navigate(Nav::Right, &mut settings, &v);
        menus.navigate(Nav::Right, &mut settings, &v);
        menus.navigate(Nav::Accept, &mut settings, &v);
        assert!(menus.settings_open);
    }

    /// a game is removed only once the player says so, and never while it
    /// runs; while any game recompiles, none starts.
    #[test]
    fn removing_a_game_is_asked_and_no_game_starts_during_a_recompile() {
        let mut settings = Settings::default();
        let mut library = Library::default();
        for (id, path) in [(7, "a.3ds"), (8, "b.3ds")] {
            library.games.push(Game { program_id: id, path: PathBuf::from(path), name: format!("Game {id}"), ..Game::default() });
        }
        let mut menus = Menus { game_settings: Some((7, "Game 7".into())), ..Menus::default() };
        let v = view(&library);
        assert!(menus.handle(Item::RemoveGame, Op::Activate, &mut settings, &v).is_empty());
        assert_eq!(menus.select_popup.as_ref().map(|popup| popup.selected), Some(1), "Keep is selected");
        assert!(menus.navigate(Nav::Accept, &mut settings, &v).is_empty());
        menus.handle(Item::RemoveGame, Op::Activate, &mut settings, &v);
        menus.navigate(Nav::Up, &mut settings, &v);
        assert_eq!(menus.navigate(Nav::Accept, &mut settings, &v), [Action::RemoveGame(7)]);

        // game 8 recompiling keeps game 7 from starting too
        let jobs = [Job::at_stage(8, Stage::Compiling { done: 1, total: 10 })];
        let busy = View { jobs: &jobs, ..view(&library) };
        let mut menus = Menus::default();
        let played = Settings { recompiled: false, ..Settings::default() };
        assert!(menus.tile(0, &played, &busy).is_empty());
        assert_eq!(menus.tile(0, &played, &v), [Action::Play(PathBuf::from("a.3ds"))]);
        let waiting = [Wanted { program_id: 8, path: PathBuf::from("b.3ds") }];
        assert!(menus.tile(0, &played, &View { waiting: &waiting, ..view(&library) }).is_empty());
    }

    /// a game not recompiled asks before it starts, with recompiling first,
    /// and stops asking once told to; another recompile goes first.
    #[test]
    fn starting_a_game_not_recompiled_asks_about_recompiling() {
        let mut settings = Settings { recompiled: true, ..Settings::default() };
        let mut library = Library::default();
        library.games.push(Game { program_id: 7, path: PathBuf::from("a.3ds"), name: "Game 7".into(), ..Game::default() });
        library.games.push(Game { program_id: 8, path: PathBuf::from("b.3ds"), name: "Game 8".into(), recompiled: true, ..Game::default() });
        let v = view(&library);
        let mut menus = Menus::default();
        assert_eq!(menus.tile(1, &settings, &v), [Action::Play(PathBuf::from("b.3ds"))], "recompiled games just start");
        assert!(menus.tile(0, &settings, &v).is_empty());
        let popup = menus.select_popup.clone().expect("it asks");
        assert_eq!(popup.options[0], "Recompile now");
        assert_eq!(menus.navigate(Nav::Accept, &mut settings, &v), [Action::Recompile(vec![library.games[0].wanted()])]);

        menus.tile(0, &settings, &v);
        menus.navigate(Nav::Down, &mut settings, &v);
        menus.navigate(Nav::Down, &mut settings, &v);
        assert_eq!(menus.navigate(Nav::Accept, &mut settings, &v), [Action::Play(PathBuf::from("a.3ds"))]);
        assert_eq!(menus.tile(0, &settings, &v), [Action::Play(PathBuf::from("a.3ds"))], "not asked again");

        // from the game's options it explains, and waits its turn
        let jobs = [Job::at_stage(8, Stage::Compiling { done: 1, total: 10 })];
        let busy = View { jobs: &jobs, ..view(&library) };
        let mut menus = Menus { game_settings: Some((7, "Game 7".into())), ..Menus::default() };
        assert!(menus.handle(Item::Recompile, Op::Activate, &mut settings, &busy).is_empty());
        let popup = menus.select_popup.clone().expect("it explains");
        assert!(popup.title.contains("ten minutes") && popup.title.contains("It starts once"));
        assert_eq!(popup.options[0], "Recompile after that");
    }

    #[test]
    fn removing_a_pack_is_asked_first_and_keep_is_the_default() {
        let mut settings = Settings::default();
        let library = Library::default();
        let v = view(&library);
        let mut menus = Menus {
            game_settings: Some((7, "A game".into())),
            packs: vec![InstalledPack { name: "HD".into(), path: "Z/mods/7/textures/HD".into(), target: None }],
            ..Menus::default()
        };
        assert!(menus.handle(Item::Pack(0), Op::Activate, &mut settings, &v).is_empty());
        let popup = menus.select_popup.clone().expect("asked first");
        assert_eq!(popup.selected, 1, "Keep is selected");
        // A on Keep does nothing
        assert!(menus.navigate(Nav::Accept, &mut settings, &v).is_empty());
        // and on Delete removes it
        menus.handle(Item::Pack(0), Op::Activate, &mut settings, &v);
        menus.navigate(Nav::Up, &mut settings, &v);
        assert_eq!(menus.navigate(Nav::Accept, &mut settings, &v), [Action::RemovePack(menus.packs[0].clone())]);
    }

    #[test]
    fn the_pause_slot_goes_round() {
        let mut settings = Settings::default();
        let library = Library::default();
        let mut menus = Menus { slot: 9, ..Menus::default() };
        menus.handle(Item::Slot, Op::Step(1), &mut settings, &view(&library));
        assert_eq!(menus.slot, 1);
        menus.handle(Item::Slot, Op::Step(-1), &mut settings, &view(&library));
        assert_eq!(menus.slot, 9);
    }

    #[test]
    fn only_some_settings_are_a_games_own() {
        assert!(game_can_own(Item::Resolution));
        assert!(game_can_own(Item::Recompiled));
        assert!(!game_can_own(Item::Volume));
        assert!(!game_can_own(Item::Fullscreen));
    }

    #[test]
    fn games_already_in_line_are_left_out() {
        let wanted = |program_id, path: &str| Wanted { program_id, path: path.into() };
        let games = || [wanted(1, "a"), wanted(2, "b"), wanted(3, "c")].into_iter();
        let ids = |games: Vec<Wanted>| games.into_iter().map(|game| game.program_id).collect::<Vec<_>>();
        assert_eq!(ids(idle(games(), &[], &[])), [1, 2, 3]);
        let twice = idle([wanted(1, "a.3ds"), wanted(2, "b"), wanted(1, "a.cci")].into_iter(), &[], &[]);
        assert_eq!(twice, [wanted(1, "a.3ds"), wanted(2, "b")]);
        assert_eq!(ids(idle(games(), &[], &[wanted(2, "b")])), [1, 3]);
        let job = Job::at_stage(1, Stage::Compiling { done: 1, total: 2 });
        assert_eq!(ids(idle(games(), std::slice::from_ref(&job), &[wanted(2, "b")])), [3]);
    }

    #[test]
    fn keys_have_short_names() {
        assert_eq!(key_name(KeyCode::KeyX), "X");
        assert_eq!(key_name(KeyCode::ArrowUp), "Up");
        assert_eq!(key_name(KeyCode::Digit1), "1");
        assert_eq!(key_name(KeyCode::Enter), "Enter");
    }
}
