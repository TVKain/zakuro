//! Zakuro's frontend, a window showing the library of games or a game with
//! a menu over it, and the loop that drives the emulated console one frame
//! at a time.

pub mod audio;
pub mod camera;
pub mod cli;
pub mod gamepad;
pub mod gui;
pub mod icons;
pub mod input;
pub mod library;
pub mod menus;
pub mod microphone;
pub mod present;
pub mod pro_controller;
pub mod recompile;
pub mod report;
pub mod nav;
pub mod settings;
pub mod store;
pub mod timing;
pub mod pacing;
pub mod zig;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

use zakuro_common::Screen;
use zakuro_core::{loader, Config, FrameOutcome, System};
use zakuro_gpu::{layout, GpuScreen, Overlay, PresentError, RendererKind, ScreenImage};

use gui::Gui;
use input::Keyboard;
use library::Library;
use menus::{Action, Menus};
use nav::{InputMode, Nav};
use present::Backend;
use recomp3ds::compile::Compiler;
use recompile::{Job, Stage, Wanted};
use settings::{CameraSource, MicrophoneSource, Renderer, Screens, Settings};

pub use zakuro_core::memory::Memory;
pub use zakuro_core::recompiled::Linked;

use crate::settings::Orientation;

/// what a program built on Zakuro shows over its game, worked out from the
/// game's memory: a line kept in a corner, and a notice shown for a moment.
/// a recompiled game can show what the game keeps hidden this way, its
/// alignment, a timer, without drawing into the game's own screens.
pub trait GameOverlay {
    /// called after each frame the game runs, with its memory.
    fn frame(&mut self, memory: &mut Memory) -> OverlayText;
}

/// what a GameOverlay shows after a frame.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct OverlayText {
    /// a line in the bottom right corner while it is some.
    pub corner: Option<String>,
    /// a notice at the top for a moment, as a saved state's.
    pub notice: Option<String>,
}

/// runs the emulator as the command line says, on recompiled code linked
/// into the program when there is some.
pub fn run(linked: Option<Linked>) {
    run_with(linked, None)
}

/// runs the emulator as run does, with an overlay over the game.
pub fn run_with(linked: Option<Linked>, overlay: Option<Box<dyn GameOverlay>>) {
    let debug = std::env::args().any(|arg| arg == "--debug" || arg == "-d");
    #[cfg(windows)]
    if debug {
        cli::attach_console();
    }
    report::init(debug);

    let options = match cli::parse() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("zakuro: {message}");
            std::process::exit(2);
        }
    };
    let mut settings = Settings::load();
    // the console's own number, made the first time and kept, which tells
    // it apart from others over local wireless
    if settings.console_id == 0 {
        use std::hash::{BuildHasher, Hasher};
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |time| time.as_nanos()));
        settings.console_id = hasher.finish() | 1;
        settings.save();
    }

    let data_dir = options.data.clone().map(PathBuf::from).or_else(default_data_dir);
    if let Some(dir) = &data_dir {
        report::to_file(dir);
        bring_saves(dir);
    }

    let mut app = App {
        keyboard: Keyboard::new(settings.keys.clone()),
        gamepads: gamepad::Gamepads::new(settings.pad.clone()),
        scale: options.scale.unwrap_or(settings.scale).max(1),
        layout: settings.layout,
        orientation: settings.orientation,
        top_shape: settings.top_shape,
        options,
        active: settings.clone(),
        started: settings.clone(),
        presented_with: None,
        settings,
        linked,
        overlay,
        overlay_corner: None,
        library: Library::new(data_dir.clone()),
        data_dir,
        game: None,
        menus: {
            let mut menus = Menus::default();
            menus.slot = 1;
            menus
        },
        jobs: Vec::new(),
        waiting: Vec::new(),
        gui: None,
        backend: None,
        window: None,
        audio: None,
        mouse_down: false,
        tilting_from: None,
        cursor: (0.0, 0.0),
        cursor_seen: false,
        last_title_update: Instant::now(),
        next_frame: Instant::now(),
        shown: 0,
        shown_fps: 60.0,
        spent: Spent::default(),
        paused: false,
        fast_forward_key: false,
        fast_forwarding: false,
        next_shown: Instant::now(),
        refresh: timing::refresh().unwrap_or(UNKNOWN_REFRESH),
        pacing: pacing::Pacing::default(),
        timing: timing::Timing::from_env(),
        stop: false,
        camera: None,
        microphone: None,
        blowing: std::sync::Arc::default(),
        room_server: None,
        room: None,
        dlp_guest: None,
        dlp_own: None,
        wireless_reboot: None,
        maximized_before_full_screen: false,
        resize_after_full_screen: false,
        install: None,
        identifying: None,
        dropped: Vec::new(),
        added: None,
        cursor_hidden: false,
    };

    if let Some(frames) = app.options.headless {
        app.start();
        if let Some(game) = &mut app.game {
            run_headless(&mut game.system, frames);
        }
        return;
    }

    app.rescan();
    app.audio = if app.options.mute {
        None
    } else {
        audio::Audio::open(zakuro_core::AUDIO_SAMPLE_RATE).inspect_err(|error| log::warn!("no sound, {error}")).ok()
    };
    app.apply_volume();
    app.apply_audio_speed();

    let event_loop = EventLoop::new().expect("create an event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    #[cfg(windows)]
    unsafe {
        #[link(name = "winmm")]
        extern "system" {
            fn timeBeginPeriod(uPeriod: u32) -> u32;
        }
        timeBeginPeriod(1);
    }

    if let Err(error) = event_loop.run_app(&mut app) {
        eprintln!("zakuro: {error}");
    }
}

/// opens a folder in the system's file manager.
fn show_folder(folder: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match std::process::Command::new(program).arg(folder).spawn() {
        // waited for so it leaves nothing behind when it closes
        Ok(mut child) => drop(std::thread::spawn(move || child.wait())),
        Err(error) => log::warn!("showing {}: {error}", folder.display()),
    }
}

/// the system's place for a program's data.
fn default_data_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    let dir = if cfg!(windows) {
        var("APPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|home| home.join("Library/Application Support"))
    } else {
        var("XDG_DATA_HOME").or_else(|| var("HOME").map(|home| home.join(".local/share")))
    };
    dir.map(|dir| dir.join("zakuro"))
}

/// how long ago a state was saved, as a person would say it.
fn ago(time: std::time::SystemTime) -> String {
    let seconds = time.elapsed().map_or(0, |elapsed| elapsed.as_secs());
    match seconds {
        0..60 => "saved just now".into(),
        60..120 => "saved a minute ago".into(),
        120..3600 => format!("saved {} minutes ago", seconds / 60),
        3600..7200 => "saved an hour ago".into(),
        7200..86400 => format!("saved {} hours ago", seconds / 3600),
        86400..172800 => "saved yesterday".into(),
        _ => format!("saved {} days ago", seconds / 86400),
    }
}

/// saves Zakuro kept in the working directory before it had a place for
/// them, copied there the first time. the old ones stay as they were.
fn bring_saves(data: &Path) {
    let (old, new) = (Path::new("user"), data.join("user"));
    if new.exists() || !old.is_dir() {
        return;
    }
    // a copy cut short is left under another name, and tried again
    let partial = data.join("user.copying");
    let copied = std::fs::remove_dir_all(&partial)
        .or_else(|error| if error.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(error) })
        .and_then(|()| copy_dir(old, &partial))
        .and_then(|()| std::fs::rename(&partial, &new));
    match copied {
        Ok(()) => log::info!("copied the saves in ./user to {}, the old ones stay as they were", new.display()),
        Err(error) => log::warn!("could not copy the saves in ./user to {}, {error}", new.display()),
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// builds a System with no title loaded and both screens filled with a
/// solid color, to check the presentation path (framebuffer config -> guest
/// memory -> window) in isolation from any title's boot logic, and that the
/// GPU starts, which a packed release is tried with.
fn paint_test_pattern(config: Config) -> System {
    use zakuro_core::memory::{MemoryRegion, MemoryState, Permission};

    let mut system = System::new(config);
    if system.config.hardware_renderer {
        match system.gpu.enable_hardware_renderer(system.config.resolution, system.config.device.clone(), None) {
            Ok(name) => log::info!("the 3D draws on {name}"),
            Err(error) => log::warn!("the 3D can't draw on the GPU, {error}"),
        }
    }

    // ABGR in memory for format 0 (Rgba8), red = 00 00 00 FF?
    let paint = |system: &mut System, screen: Screen, rgba: [u8; 4]| {
        let width = screen.width();
        let height = screen.height();
        let stride = height * 4;
        let size = stride * width;

        let block = system
            .memory
            .phys
            .allocate(MemoryRegion::Base, size)
            .expect("allocate test pattern framebuffer");

        let vaddr = zakuro_core::services::gsp::physical_to_virtual(system, block.addr);
        system.memory.map(
            vaddr,
            block.addr,
            size,
            Permission::READ | Permission::WRITE,
            MemoryState::Shared,
        );

        let mut pixel = [0u8; 4];
        pixel[3] = rgba[0]; // r
        pixel[2] = rgba[1]; // g
        pixel[1] = rgba[2]; // b
        pixel[0] = rgba[3]; // a
        let mut data = Vec::with_capacity(size as usize);
        for _ in 0..(size / 4) {
            data.extend_from_slice(&pixel);
        }
        system.memory.write_physical(block.addr, &data);

        let index = match screen {
            Screen::Top => 0,
            Screen::Bottom => 1,
        };
        system.set_framebuffer(index, 0, block.addr, block.addr, stride, 0);
    };

    paint(&mut system, Screen::Top, [220, 40, 40, 255]); // red
    paint(&mut system, Screen::Bottom, [40, 90, 220, 255]); // blue
    system
}

fn run_headless(system: &mut System, frames: u64) {
    let start = Instant::now();
    let mut outcome = FrameOutcome::Completed;
    let mut ran = 0;
    for _ in 0..frames {
        outcome = system.run_frame();
        ran += 1;
        if outcome != FrameOutcome::Completed {
            break;
        }
    }
    let elapsed = start.elapsed();
    println!("{outcome:?} after {ran} frames in {elapsed:.2?}");
    println!(
        "{:.1} MIPS, {}",
        system.cpu.cycles as f64 / elapsed.as_secs_f64() / 1e6,
        system.status_line()
    );
    if !system.fatal_errors.is_empty() {
        println!("fatal errors: {}", system.fatal_errors.join("; "));
    }
    for (thread, pc, hits) in system.hot_spots(10) {
        println!("  hot {thread:<8} 0x{pc:08X} {hits}");
    }
}

/// a game being played.
struct Running {
    system: System,
    path: PathBuf,
    name: String,
    /// the inputs written down, ZAKURO_RECORD=file, for playing the run
    /// back.
    recorder: Option<zakuro_core::replay::Recorder>,
    /// the inputs a recording wrote down, ZAKURO_REPLAY=file, played back in
    /// place of the keyboard's and the controllers', and the frames run.
    replay: Option<zakuro_core::replay::Replay>,
    frame: u64,
    /// frames run since counting_since, for the frame rate.
    frames: u32,
    counting_since: Instant,
    fps: f32,
}

impl Running {
    fn new(system: System, path: PathBuf, name: String) -> Running {
        Running {
            system,
            path,
            name,
            recorder: None,
            replay: None,
            frame: 0,
            frames: 0,
            counting_since: Instant::now(),
            fps: 0.0,
        }
    }

    fn count_frame(&mut self) {
        self.frames += 1;
        let elapsed = self.counting_since.elapsed();
        if elapsed >= Duration::from_millis(500) {
            self.fps = self.frames as f32 / elapsed.as_secs_f32();
            self.frames = 0;
            self.counting_since = Instant::now();
        }
    }
}

struct App {
    options: cli::Options,
    /// the settings as the user left them, which get saved.
    settings: Settings,
    /// the settings in force, what the running game has of its own over
    /// the general ones, which is all there is without a game.
    active: Settings,
    /// the settings the running game started with, for what it reads only
    /// as it starts.
    started: Settings,
    /// the graphics API the window was opened with, as the settings had it.
    presented_with: Option<Renderer>,
    linked: Option<Linked>,
    /// what the program built on Zakuro shows over the game, and the line it
    /// keeps in the corner now.
    overlay: Option<Box<dyn GameOverlay>>,
    overlay_corner: Option<String>,
    data_dir: Option<PathBuf>,
    game: Option<Running>,
    library: Library,
    menus: Menus,
    jobs: Vec<Job>,
    /// the games waiting for their turn to recompile, the next one first.
    waiting: Vec<Wanted>,
    gui: Option<Gui>,
    // backend must be declared (and therefore dropped) before window, Rust
    // drops struct fields in declaration order, and the GL surface's Drop
    // calls eglDestroySurface, which on Wayland does a protocol round-trip
    // against the window's wl_surface.
    backend: Option<Backend>,
    window: Option<Window>,
    keyboard: Keyboard,
    gamepads: gamepad::Gamepads,
    audio: Option<audio::Audio>,
    /// the window's size, times the console's.
    scale: u32,
    /// how the screens are arranged.
    layout: Screens,
    orientation: Orientation,
    /// how wide the top screen shows.
    top_shape: settings::TopShape,
    /// the left button is down, and where the pointer is, in window pixels.
    mouse_down: bool,
    /// where the pointer was when the right button went down, which tilts
    /// the console while held.
    tilting_from: Option<(f32, f32)>,
    cursor: (f32, f32),
    /// whether the window has been told where the pointer is yet: the first
    /// time is where it rests, not a move.
    cursor_seen: bool,
    last_title_update: Instant,
    /// when the next frame is due. frames run to a schedule rather than one
    /// after another, so that a slow one is made up by those after it and
    /// the sound, made as the console runs, keeps up.
    next_frame: Instant,
    /// frames shown since the title was last updated.
    shown: u32,
    /// measured presentation frame rate shown in the overlay.
    shown_fps: f32,
    /// how long emulating and showing frames took since the title was last
    /// updated, for the frame rate's log.
    spent: Spent,
    /// stopped with F1, without the menu.
    paused: bool,
    /// Tab is held down, for fast forward.
    fast_forward_key: bool,
    /// the game runs as fast as it can, its sound left out.
    fast_forwarding: bool,
    /// when the next picture may show at the soonest, the frame rate
    /// limit's period after the last.
    next_shown: Instant,
    /// how often the display refreshes, which is as often as Uncapped shows
    /// a picture: more often only repeats one the display never shows.
    refresh: Duration,
    /// lines frames up with the display's refreshes.
    pacing: pacing::Pacing,
    /// when frames were emulated and shown, for ZAKURO_TIMING.
    timing: Option<timing::Timing>,
    stop: bool,
    /// what the running game's cameras were given to see, to give them
    /// another only when the settings change it.
    camera: Option<(CameraSource, Option<PathBuf>)>,
    /// what the running game's microphone was given to hear, the same way.
    microphone: Option<MicrophoneSource>,
    /// set while the blowing key is held, which the microphone hears.
    blowing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// the room this program holds open, and the one it is in.
    room_server: Option<zakuro_core::room::Server>,
    room: Option<zakuro_core::room::Connection>,
    /// Download Play as a guest, while in a room with no game running.
    dlp_guest: Option<zakuro_core::download_play::Guest>,
    /// the player's own copy of the child joined for, played in place of
    /// one sent.
    dlp_own: Option<PathBuf>,
    /// the network the next game joins as it starts, the host's of a
    /// Download Play child.
    wireless_reboot: Option<[u8; 16]>,
    /// the window was maximized when it went full screen, which leaving
    /// full screen brings back.
    maximized_before_full_screen: bool,
    /// the layout or the scale changed while the window was full screen,
    /// which kept its size, so leaving full screen fits the window to them.
    resize_after_full_screen: bool,
    /// games and add-ons being put into the library.
    install: Option<store::Install>,
    /// what was dropped or picked, being looked at, and the game it was
    /// added from.
    identifying: Option<(std::sync::mpsc::Receiver<Vec<store::Found>>, Option<u64>)>,
    /// files dropped on the window since the last frame.
    dropped: Vec<PathBuf>,
    /// the games, by the unique part of their ids, that what was just
    /// added went to, to ask about recompiling once the library has them.
    added: Option<Vec<u32>>,
    /// the cursor is hidden, buttons driving the interface.
    cursor_hidden: bool,
}

/// how long each step of the loop took, its mean and its longest.
#[derive(Default)]
struct Spent {
    emulating: Times,
    showing: Times,
}

#[derive(Default)]
struct Times {
    total: Duration,
    longest: Duration,
    count: u32,
}

impl Times {
    fn add(&mut self, time: Duration) {
        self.total += time;
        self.longest = self.longest.max(time);
        self.count += 1;
    }
}

impl std::fmt::Display for Times {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mean = self.total.as_secs_f64() * 1e3 / self.count.max(1) as f64;
        write!(f, "{mean:.1} ms, at most {:.1}", self.longest.as_secs_f64() * 1e3)
    }
}

/// one 3DS frame, about 16.71 ms.
const FRAME_TIME: Duration =
    Duration::from_nanos(zakuro_core::CYCLES_PER_FRAME * 1_000_000_000 / zakuro_core::kernel::thread::CPU_CLOCK_HZ);
/// how often to show pictures when Uncapped and the display's refresh is
/// unknown.
const UNKNOWN_REFRESH: Duration = Duration::from_nanos(1_000_000_000 / 240);

/// how long a step may sleep: until the next frame is due, or the next
/// picture when one in between will show before it.
fn sleep_for(playing: bool, now: Instant, next_shown: Instant, next_frame: Instant, cadence: Duration) -> Duration {
    let until = |at: Instant| at.saturating_duration_since(now);
    if !playing {
        return until(next_shown);
    }
    let between = cadence.as_secs_f64() < FRAME_TIME.as_secs_f64() * 0.9;
    let shown = next_shown.max(now);
    if between && next_frame.saturating_duration_since(shown) > cadence / 2 {
        until(next_frame.min(shown))
    } else {
        until(next_frame)
    }
}

/// how far behind the schedule may fall before it starts over instead of
/// running fast to catch up, after a pause say.
const CATCH_UP_LIMIT: Duration = Duration::from_millis(200);
/// whether a step shows a picture now, cadence apart at most. a frame just
/// emulated shows straight away, not at the next tick of a clock of the
/// presenter's own, which would hold it back by up to a tick and, ticking
/// against the console's 59.83 frames a second, show frames unevenly; it
/// may come a quarter of a frame early, so that at 30 frames a second every
/// other one shows. between frames the same picture, or one in between with
/// frame interpolation, shows again only when the limit is above the game's
/// rate and the next frame is not about to come anyway.
fn shows_now(emulated: bool, playing: bool, now: Instant, next_shown: Instant, next_frame: Instant, cadence: Duration) -> bool {
    let between = cadence.as_secs_f64() < FRAME_TIME.as_secs_f64() * 0.9;
    if emulated {
        // at the game's rate or above every frame shows, though a picture
        // in between, or the limit's own clock, a little faster than the
        // console's, says it is early
        let keeps_up = cadence.as_secs_f64() <= FRAME_TIME.as_secs_f64() * 1.05;
        return between || keeps_up || now + FRAME_TIME / 4 >= next_shown;
    }
    if !playing {
        return now >= next_shown;
    }
    between && now >= next_shown && next_frame.saturating_duration_since(now) > cadence / 2
}

/// what the player is told when the 3D can't be drawn on the GPU.
const GPU_FALLBACK: &str = "The 3D can't be drawn on the GPU, as Vulkan isn't working on this computer, so it's drawn in software. \
Games run much slower that way, and texture packs are left out. Updating the graphics driver usually fixes it; the log says \
what went wrong. To draw in software without this message, set \"3D rendering\" to CPU in System Settings, Graphics.";

impl App {
    /// opens the window with the presenter the settings ask for, or OpenGL
    /// when Vulkan won't start, and the interface in it, false when no
    /// window could be opened.
    fn open_window(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let size = self.window_size();
        let mut attributes = Window::default_attributes().with_title("Zakuro").with_inner_size(size);
        if let Ok(image) = image::load_from_memory(include_bytes!("../../../packaging/zakuro.png")) {
            let rgba = image.to_rgba8();
            let (w, h) = rgba.dimensions();
            if let Ok(icon) = winit::window::Icon::from_rgba(rgba.into_raw(), w, h) {
                attributes = attributes.with_window_icon(Some(icon));
            }
        }
        // it always opens maximized, full screen is a toggle away
        attributes = attributes.with_maximized(true);
        if self.settings.fullscreen {
            attributes = attributes.with_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        let renderer = self.options.renderer.unwrap_or(match self.settings.renderer {
            Renderer::Vulkan => RendererKind::Vulkan,
            Renderer::OpenGl => RendererKind::OpenGl,
        });
        self.presented_with = Some(self.settings.renderer);

        let backend = Backend::create(event_loop, attributes.clone(), renderer).or_else(|error| {
            if renderer != RendererKind::Vulkan {
                return Err(error);
            }
            // a machine without a working Vulkan driver still gets a window
            log::warn!("could not start the Vulkan backend, {error}, presenting with OpenGL instead");
            Backend::create(event_loop, attributes, RendererKind::OpenGl)
        });
        match backend {
            Ok((window, mut backend)) => {
                log::info!("presenting with the {} backend", backend.name());
                backend.set_layout(self.layout.screens());
                backend.set_orientation(self.orientation.screen_orientation());
                backend.set_top_width(self.active.top_shape.width());
                backend.set_scaling(self.active.filter.screens(), self.active.integer_scale);
                backend.set_stereo(self.active.stereo());
                self.gui = Some(Gui::new(&window));
                self.window = Some(window);
                self.backend = Some(backend);
                true
            }
            Err(error) => {
                eprintln!("zakuro: could not start the {renderer:?} backend: {error}");
                log::error!("could not start the {renderer:?} backend: {error}");
                false
            }
        }
    }

    /// opens the window again with the presenter the settings now ask for,
    /// while no game runs, as a running game may draw with the presenter's
    /// device. what the interface had on the old one is uploaded again.
    fn switch_presenter(&mut self, event_loop: &ActiveEventLoop) {
        let from = self.presented_with;
        // the presenter goes before the window it draws into
        self.gui = None;
        self.backend = None;
        self.window = None;
        self.menus.forget_textures();
        if self.open_window(event_loop) {
            let name = if self.backend.as_ref().map(Backend::name) == Some("opengl") { "OpenGL" } else { "Vulkan" };
            self.menus.toast(&format!("Now drawing with {name}"));
            return;
        }
        // back to the one that worked
        if let Some(from) = from {
            self.settings.renderer = from;
            self.settings.save();
        }
        if !self.open_window(event_loop) {
            event_loop.exit();
            return;
        }
        self.menus.message = Some("That graphics API couldn't start on this computer, so Zakuro kept the one it had.".into());
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if self.open_window(event_loop) {
            // the game the command line names starts on the device the
            // presenter may lend the renderer, once there is one
            self.start();
        } else {
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // Tab is fast forward while a game runs, not a key for moving the
        // interface's focus around
        let (tab, repeat) = match &event {
            WindowEvent::KeyboardInput { event, .. } => (event.physical_key == PhysicalKey::Code(KeyCode::Tab), event.repeat),
            _ => (false, false),
        };
        let in_game = self.game.as_ref().is_some_and(|game| game.system.keyboard_request().is_none())
            && !self.menus.over_game();
        // a Tab held down keeps repeating over a message too
        let withheld = tab && (in_game || (repeat && self.game.is_some()));
        let consumed = match (&mut self.gui, &self.window) {
            (Some(gui), Some(window)) if !withheld => gui.event(window, &event),
            _ => false,
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            // keys let go while another window has the keyboard never come
            WindowEvent::Focused(false) => {
                self.fast_forward_key = false;
                self.blow(false);
                self.keyboard.release();
            }
            // games copied into the library folder by hand show up on coming back
            WindowEvent::Focused(true) if self.game.is_none() && self.install.is_none() && !self.library.is_busy() => self.rescan(),
            WindowEvent::Resized(size) => {
                if let Some(backend) = &mut self.backend {
                    backend.resize(size.width, size.height);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => self.key(event, event_loop, consumed),
            WindowEvent::DroppedFile(path) => self.dropped.push(path),
            WindowEvent::MouseWheel { .. } => self.menus.mode = InputMode::Pointer,
            WindowEvent::MouseInput { state, button, .. } => {
                self.menus.mode = InputMode::Pointer;
                if button == MouseButton::Left {
                    self.mouse_down = state == ElementState::Pressed && !self.pointer_taken(consumed);
                    self.touch();
                }
                if button == MouseButton::Right {
                    let pressed = state == ElementState::Pressed && !self.pointer_taken(consumed);
                    self.tilting_from = pressed.then_some(self.cursor);
                    self.tilt();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                // a real move hands the interface to the mouse; the few pixels
                // a resting mouse or a window change gives don't, nor does
                // the window opening under it
                let moved = (position.x as f32 - self.cursor.0).abs() + (position.y as f32 - self.cursor.1).abs();
                let seen = std::mem::replace(&mut self.cursor_seen, true);
                if moved > 3.0 && seen {
                    self.menus.mode = InputMode::Pointer;
                }
                self.cursor = (position.x as f32, position.y as f32);
                self.touch();
                self.tilt();
            }
            WindowEvent::Touch(touch) => {
                self.menus.mode = InputMode::Pointer;
                self.cursor = (touch.location.x as f32, touch.location.y as f32);
                match touch.phase {
                    TouchPhase::Started | TouchPhase::Moved => {
                        self.mouse_down = !self.pointer_taken(consumed);
                        self.touch();
                    }
                    TouchPhase::Ended | TouchPhase::Cancelled => {
                        self.mouse_down = false;
                        self.touch();
                    }
                }
            }
            WindowEvent::RedrawRequested => self.step(event_loop),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl App {
    /// starts looking at what was dropped or picked, away from the
    /// interface, to ask what to add.
    fn identify(&mut self, paths: Vec<PathBuf>, for_game: Option<u64>) {
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = send.send(store::identify(&paths));
        });
        self.identifying = Some((receive, for_game));
    }

    /// takes in what was looked at, and an install once it is done.
    fn poll_store(&mut self) {
        if !self.dropped.is_empty() {
            let dropped = std::mem::take(&mut self.dropped);
            let for_game = self.menus.game_settings.as_ref().map(|(id, _)| *id);
            self.identify(dropped, for_game);
        }
        if let Some((receive, for_game)) = &self.identifying {
            match receive.try_recv() {
                Ok(found) => {
                    let for_game = *for_game;
                    self.identifying = None;
                    if !found.is_empty() {
                        self.menus.open_import(found, &self.library, for_game);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.identifying = None,
            }
        }
        let finished = self.install.as_ref().map(store::Install::progress).filter(|progress| progress.finished);
        if let Some(progress) = finished {
            self.install = None;
            if progress.errors.is_empty() {
                self.menus.toast("Added to your library");
            } else {
                self.menus.message = Some(format!("Some of it couldn't be added:\n\n{}", progress.errors.join("\n")));
            }
            self.menus.refresh_packs(self.data_dir.as_deref());
            self.rescan();
        }
    }

    /// once the library has the games something was just added to, asks
    /// about recompiling the ones that need it.
    fn ask_about_added(&mut self) {
        // asked on Home, once no game is running
        if self.added.is_none() || self.game.is_some() || self.install.is_some() || self.library.is_busy() || self.menus.select_popup.is_some() {
            return;
        }
        let added = self.added.take().unwrap_or_default();
        let games: Vec<(Wanted, String)> = self
            .library
            .games
            .iter()
            .filter(|game| game.problem.is_none() && added.contains(&(game.program_id as u32)))
            .filter(|game| self.settings.recompiled && (!game.recompiled || game.outdated))
            .filter(|game| menus::Busy::of(game.program_id, &self.jobs, &self.waiting).is_none())
            .map(|game| (game.wanted(), game.name.clone()))
            .collect();
        if !games.is_empty() {
            self.menus.open_ask(menus::Ask::Added(games), &self.jobs, &self.waiting);
        }
    }

    /// takes a game off Home, deleting its files and those of its update
    /// and DLC from Zakuro's library. its saves and mods stay.
    fn remove_game(&mut self, program_id: u64) {
        let running = self.program_id() == Some(program_id);
        let busy = self.jobs.iter().any(|job| job.program_id == program_id && !job.stage().finished());
        let Some(game) = self.library.games.iter().find(|game| game.program_id == program_id && game.problem.is_none()) else { return };
        let Some(library) = store::library_dir(self.data_dir.as_deref()) else { return };
        if running || busy {
            return;
        }
        let name = game.name.clone();
        let files = store::game_files(game, &store::sources(self.data_dir.as_deref()));
        self.waiting.retain(|waiting| waiting.program_id != program_id);
        match store::remove_game(&files, &library) {
            Ok(_) => {
                self.menus.close_modals();
                self.menus.toast(&format!("Removed {name}"));
            }
            Err(error) => self.menus.message = Some(format!("{name} couldn't be removed: {error}")),
        }
        self.rescan();
    }

    /// looks through the library's folders again.
    fn rescan(&mut self) {
        let sources = store::sources(self.data_dir.as_deref());
        if let Some(own) = sources.first() {
            // Zakuro's own library is there from the start, to add to
            let _ = std::fs::create_dir_all(&own.folder);
        }
        self.library.scan(sources);
    }

    /// the id of the running game, none without one, or with the test
    /// pattern.
    fn program_id(&self) -> Option<u64> {
        Some(self.game.as_ref()?.system.title.as_ref()?.program_id())
    }

    /// the console's settings for a game, from the command line and the
    /// settings it plays with.
    fn config(&self, settings: &Settings) -> Config {
        let interpret = self.options.interpreter || !settings.recompiled;
        Config {
            new3ds: self.options.new3ds,
            data_dir: self.data_dir.clone(),
            language: settings.language.code(),
            recompiled: self.options.recompiled.clone().filter(|_| !interpret).map(Into::into),
            linked: self.linked.filter(|_| !interpret),
            find_recompiled: !interpret,
            hardware_renderer: self.options.hardware_rasterizer.unwrap_or(settings.hardware_rasterizer),
            texture_packs: settings.texture_packs,
            interpolate: settings.frame_interpolation,
            enhancements: settings
                .enhancements
                .iter()
                .filter_map(|(id, names)| Some((u64::from_str_radix(id, 16).ok()?, names.clone())))
                .collect(),
            resolution: settings.resolution,
            device: self.backend.as_ref().and_then(Backend::shared_device),
            username: self.nickname(),
            console_id: settings.console_id,
            slider_3d: settings.slider_3d(),
            ..Config::default()
        }
    }

    /// starts what the command line asks for, the test pattern or a game.
    fn start(&mut self) {
        if self.options.test_pattern {
            // the test pattern has no CPU program to run, paused keeps just
            // presenting what is there
            let system = paint_test_pattern(self.config(&self.settings));
            self.game = Some(Running::new(system, PathBuf::new(), "test pattern".to_owned()));
            self.paused = true;
        } else if let Some(rom) = self.options.rom.clone() {
            if let Err(error) = self.play(Path::new(&rom)) {
                eprintln!("zakuro: {error}");
                std::process::exit(1);
            }
        }
        if self.options.profile {
            if let Some(game) = &mut self.game {
                game.system.enable_profiler();
            }
        }
    }

    /// starts the game at path, in place of any other, with what it has of
    /// its own in the settings.
    fn play(&mut self, path: &Path) -> Result<(), String> {
        // the old game and what it holds on the GPU go first, its sound too,
        // and the overlay's line about it
        self.game = None;
        self.overlay_corner = None;
        if let Some(audio) = &self.audio {
            audio.clear();
        }
        // a Download Play guest goes with the game
        self.dlp_guest = None;
        // one from the command line isn't in the library, or not yet, and
        // a Download Play child listed apart from its game is there with no id
        let known = self.library.games.iter().find(|game| game.path == path && game.problem.is_none()).map(|game| game.program_id);
        let program_id = known.or_else(|| Some(zakuro_fs::Title::load(path).ok()?.program_id()));
        self.active = self.settings.for_game(program_id);
        let started = self.start_game(path);
        if started.is_err() {
            self.active = self.settings.clone();
        } else {
            self.started = self.active.clone();
        }
        self.apply_layout();
        self.apply_audio_speed();
        started
    }

    /// loads the game at path, which the settings in force are for.
    fn start_game(&mut self, path: &Path) -> Result<(), String> {
        let mut config = self.config(&self.active);
        config.wireless_reboot = self.wireless_reboot.take();
        // the update, DLC and Download Play child found with the game in
        // the library
        if let Some(game) = self.library.games.iter().find(|game| game.path == path) {
            config.update = game.update.as_ref().map(|update| update.path.clone());
            config.dlc = game.dlc.iter().map(|dlc| dlc.path.clone()).collect();
            config.dlp_child = game.dlp_child.as_ref().map(|child| child.path.clone());
        } else if let Some((update, dlc, dlp_child)) = library::addons_beside(path) {
            // one from the command line starts before the library's scan is
            // in, or isn't in its folder: what's next to it goes with it
            config.update = update.map(|update| update.path);
            config.dlc = dlc.into_iter().map(|dlc| dlc.path).collect();
            config.dlp_child = dlp_child.map(|child| child.path);
        }
        // a recording starts the clock at a known time, for playing it back
        let record = std::env::var_os("ZAKURO_RECORD").map(PathBuf::from);
        let clock = zakuro_core::memory::config::host_clock();
        if record.is_some() {
            config.clock = Some(clock);
        }
        // and plays back from the clock it was made at
        let replay = match std::env::var_os("ZAKURO_REPLAY").map(PathBuf::from) {
            Some(file) => {
                let replay = zakuro_core::replay::Replay::open(&file).map_err(|error| format!("could not read {}, {error}", file.display()))?;
                log::info!("playing back the inputs in {}", file.display());
                config.clock = Some(replay.clock());
                Some(replay)
            }
            None => None,
        };
        let system = loader::load(path, config).map_err(|error| format!("could not open {}, {error}", path.display()))?;
        let outdated = system.recompiled.as_ref().is_some_and(|code| code.outdated());
        // the settings ask for the 3D on the GPU, but Vulkan didn't start:
        // only the log said so, while the game ran slowly
        let in_software = system.config.hardware_renderer && !system.gpu.draws_on_gpu();
        let name = self
            .library
            .games
            .iter()
            .find(|game| game.path == path)
            .map(|game| game.name.clone())
            .or_else(|| Some(library::Smdh::parse(system.title.as_ref()?.exefs_file("icon")?)?.name).filter(|name| !name.is_empty()))
            .or_else(|| path.file_stem().map(|stem| stem.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let mut game = Running::new(system, path.to_owned(), name);
        game.system.gpu.set_smooth_textures(self.active.smooth_textures);
        game.replay = replay;
        if let Some(record) = record {
            match zakuro_core::replay::Recorder::create(&record, clock) {
                Ok(recorder) => {
                    log::info!("recording the inputs to {}", record.display());
                    game.recorder = Some(recorder);
                }
                Err(error) => log::warn!("could not record to {}, {error}", record.display()),
            }
        }
        // both can be so, and the GPU's is what explains a slow game
        let notes: Vec<String> = [
            in_software.then(|| GPU_FALLBACK.to_owned()),
            outdated.then(|| {
                format!(
                    "{} was recompiled by an older version of Zakuro. Recompile it again from the library to get the newest improvements.",
                    game.name
                )
            }),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !notes.is_empty() {
            self.menus.message = Some(notes.join("\n\n"));
        }
        self.game = Some(game);
        // the new game's cameras see what the settings say, and it plays
        // in the room the player is in
        self.camera = None;
        self.apply_camera();
        self.microphone = None;
        self.apply_microphone();
        self.link_room();
        // nothing left open holds the game back
        self.menus.menu_open = false;
        self.menus.settings_open = false;
        self.menus.game_settings = None;
        self.keyboard.release();
        self.paused = false;
        self.next_frame = Instant::now();
        Ok(())
    }

    fn back_to_library(&mut self) {
        self.game = None;
        self.overlay_corner = None;
        if let Some(room) = &self.room {
            room.playing("");
        }
        if let Some(audio) = &self.audio {
            audio.clear();
        }
        self.menus.menu_open = false;
        self.keyboard.release();
        // what the game had of its own goes with it
        self.active = self.settings.clone();
        self.apply_layout();
    }

    /// arranges the screens another way, for the running game alone when it
    /// has an arrangement of its own.
    fn set_layout(&mut self, layout: Screens) {
        let own = self.program_id().and_then(|id| self.settings.per_game.get_mut(&settings::game_key(id)));
        match own.filter(|own| own.layout.is_some()) {
            Some(own) => own.layout = Some(layout),
            None => self.settings.layout = layout,
        }
        self.settings.save();
        self.active = self.settings.for_game(self.program_id());
        self.apply_layout();
    }

    fn key(&mut self, event: KeyEvent, event_loop: &ActiveEventLoop, consumed: bool) {
        let _ = event_loop;
        let pressed = event.state == ElementState::Pressed;
        let PhysicalKey::Code(code) = event.physical_key else { return };
        // a controller binding listening leaves it as it was on Escape
        if pressed && code == KeyCode::Escape && self.menus.rebinding_pad.take().is_some() {
            return;
        }
        // a key binding listening takes the key, Escape leaves it as it was
        if let (true, Some(index)) = (pressed, self.menus.rebinding) {
            self.menus.rebinding = None;
            if code != KeyCode::Escape {
                if let Some((_, key)) = self.settings.keys.all_mut().into_iter().nth(index) {
                    *key = code;
                }
                self.apply_settings();
            }
            return;
        }

        // the interface has the keyboard while it shows, on Home and over a
        // game with the menu or a window open, and the game none of it
        if self.game.is_none() || self.menus.over_game() {
            if !pressed {
                self.keyboard.key(event.physical_key, false);
                return;
            }
            if code == KeyCode::F11 && !event.repeat {
                self.toggle_fullscreen();
                return;
            }
            // a field being typed in keeps its keys, and Escape leaves it
            if self.gui.as_ref().is_some_and(Gui::wants_keyboard) {
                if code == KeyCode::Escape {
                    if let Some(gui) = &mut self.gui {
                        gui.clear_focus();
                    }
                }
                return;
            }
            if let Some(nav) = Nav::from_key(code).filter(|nav| !event.repeat || nav.repeats()) {
                self.menus.mode = InputMode::Directional;
                self.menus.pad_hints = false;
                self.menus.press(nav);
            }
            return;
        }

        // --- Active Gameplay Key Handling (when NOT in menu/settings) ---
        if code == KeyCode::Tab && !event.repeat && !self.settings.keys.binds(KeyCode::Tab) {
            // fast forward while it is held
            self.fast_forward_key = pressed && self.game.is_some();
        }
        if pressed && !event.repeat {
            match code {
                KeyCode::Escape if self.game.is_some() => {
                    self.menus.menu_open = !self.menus.menu_open;
                    self.keyboard.release();
                    self.gamepads.suppress_current_held();
                    if let Some(game) = &mut self.game {
                        game.system.set_input(zakuro_core::services::hid::InputState::default());
                    }
                    return;
                }
                KeyCode::F1 => {
                    self.paused = !self.paused;
                    log::info!("{}", if self.paused { "paused" } else { "resumed" });
                }
                KeyCode::F9 => self.set_layout(self.active.layout.next()),
                KeyCode::F10 if self.active.layout.other_screen() != self.active.layout => self.set_layout(self.active.layout.other_screen()),
                KeyCode::F11 => self.toggle_fullscreen(),
                KeyCode::F5 if self.game.is_some() => self.save_state(),
                KeyCode::F8 if self.game.is_some() => self.load_state(),
                _ => {}
            }
        }
        let to_game = self.game.is_some()
            && !consumed
            && !self.gui.as_ref().is_some_and(Gui::wants_keyboard);
        // blowing into the microphone while M is held
        if code == KeyCode::KeyM && !event.repeat && !self.settings.keys.binds(KeyCode::KeyM) && (to_game || !pressed) {
            self.blow(pressed);
        }
        // letting go always gets through, so that no button stays held
        if to_game || !pressed {
            self.keyboard.key(event.physical_key, pressed);
        }
    }

    /// takes in the controllers' buttons: the Home button opens and closes
    /// the menu over a game, and the rest move around the interface while
    /// it shows.
    fn poll_gamepads(&mut self, event_loop: &ActiveEventLoop) {
        let _ = event_loop;
        for button in self.gamepads.poll() {
            // a binding listening takes the button
            if let Some(index) = self.menus.rebinding_pad.take() {
                if let Some((_, bound)) = self.settings.pad.all_mut().into_iter().nth(index) {
                    *bound = button;
                }
                self.apply_settings();
                // the press that bound it doesn't also move around
                self.gamepads.suppress_current_held();
                continue;
            }
            self.menus.mode = InputMode::Directional;
            self.menus.pad_hints = true;
            if (button == self.settings.pad.menu || button == gilrs::Button::Mode) && self.game.is_some() {
                if self.menus.modal_open() {
                    self.menus.close_modals();
                } else {
                    self.menus.menu_open = !self.menus.menu_open;
                }
                self.release_input();
                continue;
            }
            if self.game.is_some() && !self.menus.over_game() {
                continue;
            }
            // the multiplayer window and a cheat being typed are forms, which
            // egui's focus moves through
            let form = self.menus.multiplayer_open || self.gui.as_ref().is_some_and(Gui::wants_keyboard);
            if form && self.menus.select_popup.is_none() && self.menus.message.is_none() {
                if let Some(gui) = &mut self.gui {
                    match button {
                        gilrs::Button::DPadDown => {
                            gui.move_focus_next();
                            continue;
                        }
                        gilrs::Button::DPadUp => {
                            gui.move_focus_prev();
                            continue;
                        }
                        gilrs::Button::South => {
                            gui.send_key(egui::Key::Enter);
                            continue;
                        }
                        _ => {}
                    }
                }
            }
            if let Some(nav) = Nav::from_button(button) {
                self.menus.press(nav);
            }
        }
    }

    /// lets go of everything held, for the game, whose buttons held to
    /// leave the menu don't count until let go.
    fn release_input(&mut self) {
        self.keyboard.release();
        self.gamepads.suppress_current_held();
        if let Some(game) = &mut self.game {
            game.system.set_input(zakuro_core::services::hid::InputState::default());
        }
    }

    /// whether the pointer is busy with something other than the game.
    fn pointer_taken(&self, consumed: bool) -> bool {
        consumed || self.game.is_none() || self.menus.over_game() || self.gui.as_ref().is_some_and(Gui::wants_pointer)
    }

    /// touches the bottom screen while the button is down over it.
    fn touch(&mut self) {
        let Some(window) = &self.window else { return };
        let size = window.inner_size();
        let (_, bottom) = layout(size.width, size.height, self.layout.screens(), self.orientation.screen_orientation(), self.top_shape.width(), self.active.integer_scale);
        // with the top screen alone there is nothing to touch
        let Some(bottom) = bottom else {
            self.keyboard.touch(None);
            return;
        };
        let x = self.cursor.0 - bottom.x;
        let y = self.cursor.1 - bottom.y;
        let inside = x >= 0.0 && y >= 0.0 && x < bottom.width && y < bottom.height;
        self.keyboard.touch((self.mouse_down && inside).then(|| {
            let (u, v) = (x / bottom.width, y / bottom.height);
            let (u, v) = match self.orientation {
                Orientation::North => (u, v),
                Orientation::East => (v, 1.0 - u),
                Orientation::South => (1.0 - u, 1.0 - v),
                Orientation::West => (1.0 - v, u),
            };
            (((u * 320.0) as u16).min(319), ((v * 240.0) as u16).min(239))
        }));
    }

    /// tilts the console while the right button is held, toward where the
    /// pointer went and by how far, as Citra's motion emulation does,
    /// which titles were tried with. letting go stands it upright again.
    fn tilt(&mut self) {
        // radians a pixel, and the furthest it goes, a quarter turn
        const PER_PIXEL: f32 = 0.01;
        let tilt = match (self.tilting_from, &self.window) {
            (Some((x, y)), Some(window)) => {
                let scale = window.scale_factor() as f32;
                let (dx, dy) = ((self.cursor.0 - x) / scale, (self.cursor.1 - y) / scale);
                let distance = dx.hypot(dy);
                let angle = (distance * PER_PIXEL).min(std::f32::consts::FRAC_PI_2);
                if distance > 0.0 { [dx / distance * angle, dy / distance * angle] } else { [0.0; 2] }
            }
            _ => [0.0; 2],
        };
        self.keyboard.tilt(tilt);
    }

    /// takes the window in or out of full screen, out of it back to how it
    /// was before, maximized unless it had been made smaller.
    fn toggle_fullscreen(&mut self) {
        if let Some(window) = &self.window {
            let full = window.fullscreen().is_some();
            if !full {
                self.maximized_before_full_screen = window.is_maximized();
                self.resize_after_full_screen = false;
                window.set_fullscreen(Some(Fullscreen::Borderless(None)));
            } else {
                window.set_fullscreen(None);
                window.set_maximized(self.maximized_before_full_screen);
                if !self.maximized_before_full_screen && self.resize_after_full_screen {
                    let _ = window.request_inner_size(self.window_size());
                }
            }
            self.settings.fullscreen = !full;
            self.active.fullscreen = !full;
        }
    }

    fn apply_volume(&self) {
        if let Some(audio) = &self.audio {
            audio.set_volume(if self.settings.mute { 0.0 } else { self.settings.volume });
        }
    }

    fn apply_audio_speed(&self) {
        if let Some(audio) = &self.audio {
            // lined up with the display, the game runs at its rate
            audio.set_speed(pacing::speed(FRAME_TIME, self.refresh));
        }
    }

    /// saves the settings and puts to use what can change right away.
    fn apply_settings(&mut self) {
        self.settings.save();
        self.active = self.settings.for_game(self.program_id());
        if let Some(game) = &mut self.game {
            if game.system.config.resolution != self.active.resolution {
                let actual = game.system.set_resolution(self.active.resolution);
                log::info!("internal resolution changed to {actual}x");
            }
            if game.system.config.texture_packs != self.active.texture_packs {
                let drawn = game.system.set_texture_packs(self.active.texture_packs);
                log::info!("texture packs {}", if drawn { "on" } else { "off" });
            }
            if game.system.gpu.smooths_textures() != self.active.smooth_textures {
                game.system.gpu.set_smooth_textures(self.active.smooth_textures);
            }
            if game.system.gpu.interpolates() != self.active.frame_interpolation {
                let on = game.system.set_interpolation(self.active.frame_interpolation);
                log::info!("frame interpolation {}", if on { "on" } else { "off" });
            }
            // games read the 3D slider every frame
            let slider = self.active.slider_3d();
            if game.system.config.slider_3d != slider {
                game.system.set_slider_3d(slider);
            }
        }
        self.apply_camera();
        self.apply_microphone();
        self.keyboard.set_keys(self.settings.keys.clone());
        self.gamepads.set_buttons(self.settings.pad.clone());
        self.apply_volume();
        self.apply_layout();
        self.apply_audio_speed();
        self.next_frame = Instant::now();
        self.next_shown = Instant::now();
    }

    /// gives the running game's cameras what the settings say they see,
    /// when it is not what they have.
    fn apply_camera(&mut self) {
        let wanted = (self.settings.camera, self.settings.camera_picture.clone());
        let Some(game) = &mut self.game else { return };
        if self.camera.as_ref() == Some(&wanted) {
            return;
        }
        game.system.camera = match &wanted {
            (CameraSource::Webcam, _) => Some(Box::new(camera::Webcam::default())),
            (CameraSource::Picture, Some(path)) => match camera::picture(path) {
                Ok(picture) => {
                    log::info!("the cameras see {}", path.display());
                    Some(Box::new(picture))
                }
                Err(error) => {
                    self.menus.message = Some(error);
                    None
                }
            },
            _ => None,
        };
        self.camera = Some(wanted);
    }

    /// gives the running game's microphone what the settings say it hears,
    /// when it is not what it has.
    fn apply_microphone(&mut self) {
        let wanted = self.settings.microphone;
        let Some(game) = &mut self.game else { return };
        if self.microphone == Some(wanted) {
            return;
        }
        let device = wanted == MicrophoneSource::Computer;
        let mut microphone = microphone::HostMicrophone::new(device, self.blowing.clone());
        // a game listening already goes on listening to the new one
        if game.system.services.mic.is_sampling() {
            zakuro_core::microphone::Microphone::sampling(&mut microphone, true);
        }
        game.system.microphone = Some(Box::new(microphone));
        self.microphone = Some(wanted);
    }

    /// starts or stops blowing into the microphone.
    fn blow(&mut self, on: bool) {
        if self.blowing.swap(on, std::sync::atomic::Ordering::Relaxed) != on {
            log::debug!("{} into the microphone", if on { "blowing" } else { "no longer blowing" });
        }
    }

    /// puts the screens' arrangement and the window scale to use, sizing
    /// the window to them.
    fn apply_layout(&mut self) {
        if let Some(backend) = &mut self.backend {
            backend.set_top_width(self.active.top_shape.width());
            backend.set_scaling(self.active.filter.screens(), self.active.integer_scale);
            backend.set_stereo(self.active.stereo());
        }
        let scale = if self.options.scale.is_none() { self.active.scale.max(1) } else { self.scale };
        if scale != self.scale || self.active.layout != self.layout || self.active.orientation != self.orientation || self.active.top_shape != self.top_shape {
            self.scale = scale;
            self.layout = self.active.layout;
            self.orientation = self.active.orientation;
            self.top_shape = self.active.top_shape;
            if let Some(backend) = &mut self.backend {
                backend.set_layout(self.layout.screens());
                backend.set_orientation(self.orientation.screen_orientation());
            }
            // a full screen or maximized window keeps its size, resizing
            // it would take it out of that, full screen fits it on leaving
            match &self.window {
                Some(window) if window.fullscreen().is_some() => self.resize_after_full_screen = true,
                Some(window) if !window.is_maximized() => {
                    let _ = window.request_inner_size(self.window_size());
                }
                _ => {}
            }
        }
    }

    /// the window size that fits the screens at the chosen scale.
    fn window_size(&self) -> winit::dpi::Size {
        let (width, height) = self.layout.screens().size(self.top_shape.width());
        let (width, height) = match self.orientation.screen_orientation().is_portrait() {
            true => (height, width),
            false => (width, height)
        };
        // scaled by whole numbers, a desktop scaled by 125% say would leave a
        // border, so the window gets whole multiples of the screens' pixels
        if let (true, Some(window)) = (self.active.integer_scale, &self.window) {
            let times = (self.scale as f64 * window.scale_factor()).round().max(1.0) as u32;
            return winit::dpi::PhysicalSize::new(width * times, height * times).into();
        }
        winit::dpi::LogicalSize::new(width * self.scale, height * self.scale).into()
    }

    /// what to recompile with: the Zig that comes with Zakuro, or else the
    /// computer's own C compiler. with neither it says what is missing.
    fn toolchain(&mut self) -> Option<Compiler> {
        if let Some(compiler) = zig::bundled(self.data_dir.as_deref()) {
            return Some(compiler);
        }
        match recomp3ds::compile::check(None) {
            Ok(compiler) => Some(compiler),
            Err(error) => {
                self.menus.message = Some(format!(
                    "Recompiling needs a C compiler. Run Zakuro setup again with \"Set up recompiling\" ticked: {error}."
                ));
                None
            }
        }
    }

    /// puts games in line to recompile, after those there are, each with
    /// its turn once the one before it is done.
    fn recompile(&mut self, games: Vec<Wanted>) {
        if self.toolchain().is_none() {
            return;
        }
        for game in games {
            let running = self.jobs.iter().any(|job| job.program_id == game.program_id && !job.stage().finished());
            if !running && !self.waiting.iter().any(|waiting| waiting.program_id == game.program_id) {
                self.waiting.push(game);
            }
        }
        self.next_recompile();
    }

    /// starts the game next in line, when none is recompiling.
    fn next_recompile(&mut self) {
        if self.waiting.is_empty() || self.jobs.iter().any(|job| !job.stage().finished()) {
            return;
        }
        // a compiler can have stopped being there meanwhile
        let Some(toolchain) = self.toolchain() else {
            self.waiting.clear();
            return;
        };
        let wanted = self.waiting.remove(0);
        self.jobs.retain(|job| job.program_id != wanted.program_id);
        // a scan may have taken it out of the library meanwhile
        if let Some(game) = self.library.games.iter().find(|game| game.path == wanted.path) {
            let update = game.update.as_ref().map(|update| update.path.as_path());
            self.jobs.push(Job::start(&game.path, update, game.program_id, &game.name, self.data_dir.as_deref(), toolchain));
        }
    }

    /// tells about recompiles as they finish, and starts the next.
    fn poll_jobs(&mut self) {
        let mut finished = Vec::new();
        for job in &mut self.jobs {
            if !job.announced && job.stage().finished() {
                job.announced = true;
                finished.push((job.program_id, job.name.clone(), job.stage()));
            }
        }
        for (program_id, name, stage) in finished {
            match stage {
                Stage::Done => {
                    self.library.recompiled(program_id);
                    // a game told to run on the interpreter stays on it
                    let playing = self.game.as_ref().is_some_and(|game| {
                        game.system.config.find_recompiled && game.system.title.as_ref().is_some_and(|title| title.program_id() == program_id)
                    });
                    let installed = zakuro_core::recompiled::installed(program_id);
                    if let (true, Some(game), Some(path)) = (playing, self.game.as_mut(), installed) {
                        // from here on the game runs on the new code, no reset needed
                        match zakuro_core::loader::swap_recompiled(&mut game.system, &path) {
                            Ok(()) => self.menus.message = Some(format!("{name} is recompiled, and it runs on the new code now.")),
                            Err(error) => {
                                log::warn!("could not switch {name} to its new code, {error}");
                                self.menus.message = Some(format!("{name} is recompiled. Reset it from the menu, Esc, to run it on the new code."));
                            }
                        }
                    }
                }
                // others can fail before anyone reads this, in a long line
                Stage::Failed(error) if error != "cancelled" => {
                    let failed = format!("Recompiling {name} failed, {error}");
                    self.menus.message = Some(self.menus.message.take().map_or(failed.clone(), |earlier| format!("{earlier}\n\n{failed}")));
                }
                _ => {}
            }
        }
        self.next_recompile();
    }

    fn act(&mut self, action: Action, event_loop: &ActiveEventLoop) {
        match action {
            Action::Play(_) if let Some(name) = recompile::blocking_play(&self.jobs, &self.waiting) => {
                self.menus.message = Some(format!("Games can be started once {name} finishes recompiling, which takes the whole processor."));
            }
            Action::Play(path) => {
                if let Err(error) = self.play(&path) {
                    self.menus.message = Some(error);
                }
            }
            Action::Recompile(games) => self.recompile(games),
            Action::Mods(program_id) => {
                if let Some(data_dir) = &self.data_dir {
                    let folder = zakuro_core::mods::dir(data_dir, program_id);
                    let made = std::fs::create_dir_all(folder.join("romfs")).and_then(|()| std::fs::create_dir_all(folder.join("textures")));
                    match made {
                        Ok(()) => show_folder(&folder),
                        Err(error) => self.menus.message = Some(format!("The mods folder can't be made: {error}")),
                    }
                }
            }
            Action::CancelRecompile(program_id) => {
                self.waiting.retain(|waiting| waiting.program_id != program_id);
                for job in self.jobs.iter().filter(|job| job.program_id == program_id) {
                    job.cancel();
                }
            }
            Action::CancelWaiting => self.waiting.clear(),
            Action::SaveState => self.save_state(),
            Action::LoadState => self.load_state(),
            Action::HostRoom => self.host_room(),
            Action::JoinRoom => {
                let address = self.settings.room_address.clone();
                self.join_room(&address);
            }
            Action::LeaveRoom => self.leave_room(),
            Action::DlpJoin(index) => self.join_download_play(index),
            Action::DlpLeave => {
                if let Some(guest) = &mut self.dlp_guest {
                    guest.leave();
                }
                self.dlp_own = None;
            }
            Action::ChooseCameraPicture => {
                let picked = rfd::FileDialog::new()
                    .set_title("A picture for the cameras")
                    .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "bmp"])
                    .pick_file();
                if let Some(file) = picked {
                    self.settings.camera = CameraSource::Picture;
                    self.settings.camera_picture = Some(file);
                    self.apply_settings();
                }
            }
            Action::ChooseBackground => {
                let picked = rfd::FileDialog::new()
                    .set_title("A picture for the library")
                    .add_filter("Pictures", &["png", "jpg", "jpeg", "webp", "bmp"])
                    .pick_file();
                if let Some(file) = picked {
                    self.settings.background = Some(file);
                    self.settings.save();
                }
            }
            Action::AddContent(for_game) => {
                let picked = rfd::FileDialog::new()
                    .set_title("Add games, updates, DLC, texture packs or cheats")
                    .add_filter("Games and content", &["3ds", "cci", "cxi", "cia", "zip", "txt"])
                    .pick_files();
                if let Some(files) = picked {
                    self.identify(files, for_game);
                }
            }
            Action::AddTextures(program_id) => {
                if let Some(folder) = rfd::FileDialog::new().set_title("The texture pack's folder").pick_folder() {
                    self.identify(vec![folder], Some(program_id));
                }
            }
            Action::Install(choice) => match (&self.data_dir, store::library_dir(self.data_dir.as_deref())) {
                (Some(data_dir), Some(library)) => {
                    let (steps, bytes) = store::plan(&choice, &library, data_dir);
                    if !steps.is_empty() {
                        self.install = Some(store::Install::start(steps, bytes));
                        // a game, its update or its DLC added changes what
                        // recompiling it makes, asked about once it's in
                        let titles = choice.found.iter().filter_map(|(found, _)| match found {
                            store::Found::Title { title_id, .. } => Some(*title_id as u32),
                            _ => None,
                        });
                        self.added.get_or_insert_with(Vec::new).extend(titles);
                    } else {
                        self.menus.toast("Nothing new to add");
                    }
                }
                _ => self.menus.message = Some("Zakuro has no data folder to keep games in.".into()),
            },
            Action::CancelInstall => {
                if let Some(install) = &self.install {
                    install.cancel();
                }
            }
            Action::RemovePack(pack) => {
                match store::remove_pack(&pack) {
                    Ok(()) => {
                        let text = format!("Removed {}", pack.name);
                        self.menus.toast(&text);
                    }
                    Err(error) => self.menus.message = Some(format!("{} couldn't be removed: {error}", pack.name)),
                }
                self.menus.refresh_packs(self.data_dir.as_deref());
                self.rescan();
            }
            Action::RemoveGame(program_id) => self.remove_game(program_id),
            Action::Resume => {
                self.menus.menu_open = false;
                self.release_input();
            }
            Action::Reset => {
                if let Some(path) = self.game.as_ref().map(|game| game.path.clone()) {
                    if let Err(error) = self.play(&path) {
                        self.menus.message = Some(error);
                    }
                }
            }
            Action::Library => self.back_to_library(),
            Action::Fullscreen => self.toggle_fullscreen(),
            Action::CopyReport => {
                if let (Some(game), Some(gui)) = (&self.game, &self.gui) {
                    gui.ctx.copy_text(report::text(&game.name, &game.system));
                    self.menus.message = Some("The game's details and the end of the log are copied. Paste them in your report and fill in what happens.".into());
                }
            }
            Action::Cheat(index, on) => {
                let Some(game) = &mut self.game else { return };
                let program_id = game.system.title.as_ref().map_or(0, |title| title.program_id());
                let Some(cheat) = game.system.cheats.get_mut(index) else { return };
                cheat.enabled = on;
                log::info!("{} {} {}", if cheat.builtin { "enhancement" } else { "cheat" }, cheat.name, if on { "on" } else { "off" });
                if cheat.builtin {
                    // an enhancement stays on for the game in the settings
                    let names = self.settings.enhancements.entry(format!("{program_id:016X}")).or_default();
                    names.retain(|name| *name != cheat.name);
                    if on {
                        names.push(cheat.name.clone());
                    }
                    self.settings.save();
                } else {
                    self.save_cheats();
                }
            }
            Action::AddCheat(name, code) => {
                let lines: Vec<String> = code.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect();
                if lines.iter().any(|line| !zakuro_core::cheats::is_code(line)) {
                    self.menus.message = Some("Some of those lines aren't codes like 00000000 00000000, the cheat skips them.".into());
                }
                if let Some(game) = &mut self.game {
                    let name = match name.trim() {
                        "" => format!("Cheat {}", game.system.cheats.len() + 1),
                        name => name.to_owned(),
                    };
                    let mut cheat = zakuro_core::cheats::Cheat::new(&name, lines, Vec::new());
                    cheat.enabled = true;
                    game.system.cheats.push(cheat);
                }
                self.save_cheats();
            }
            Action::ReloadCheats => {
                if let (Some(path), Some(game)) = (self.cheats_path(), &mut self.game) {
                    let enhancements: Vec<_> = game.system.cheats.drain(..).filter(|cheat| cheat.builtin).collect();
                    game.system.cheats = zakuro_core::cheats::load(&path);
                    game.system.cheats.extend(enhancements);
                }
            }
            Action::CheatsFolder => {
                if let Some(folder) = self.cheats_path().and_then(|path| path.parent().map(Path::to_owned)) {
                    match std::fs::create_dir_all(&folder) {
                        Ok(()) => show_folder(&folder),
                        Err(error) => self.menus.message = Some(format!("The cheats folder can't be made: {error}")),
                    }
                }
            }
            Action::Quit => event_loop.exit(),
            Action::Settings => self.apply_settings(),
            Action::Keyboard(text, button) => {
                if let Some(game) = &mut self.game {
                    game.system.answer_keyboard(&text, button);
                }
            }
        }
    }

    fn step(&mut self, event_loop: &ActiveEventLoop) {
        if self.stop {
            event_loop.exit();
            return;
        }
        // a graphics API chosen on Home opens the window again with it
        if self.options.renderer.is_none() && self.game.is_none() && self.presented_with.is_some_and(|with| with != self.settings.renderer) {
            self.switch_presenter(event_loop);
            return;
        }
        let now = Instant::now();
        if now > self.next_frame + CATCH_UP_LIMIT {
            self.next_frame = now;
        }
        self.library.poll();
        self.poll_jobs();
        self.poll_gamepads(event_loop);
        // a room that went away takes the game's local wireless with it
        if self.room.as_ref().is_some_and(|room| !room.open()) {
            self.leave_room();
            self.menus.message = Some("The room closed, or the connection to it was lost.".into());
        }
        self.download_play();

        // a game waiting on its keyboard waits for the user, not running
        let typing = self.game.as_ref().is_some_and(|game| game.system.keyboard_request().is_some());
        let playing = self.game.is_some() && !self.paused && !self.menus.over_game() && !typing;
        if self.menus.over_game() {
            if let Some(game) = &mut self.game {
                game.system.set_input(zakuro_core::services::hid::InputState::default());
            }
        }
        // Tab or a controller's right trigger, or the right stick's click
        // while ZR has the trigger, whichever no button of the console's is on
        let free = |button: gilrs::Button| self.settings.pad.map().iter().all(|&(_, bound)| bound != button);
        let trigger = [gilrs::Button::RightTrigger2, gilrs::Button::RightThumb].into_iter().find(|&button| free(button));
        let fast = playing && (self.fast_forward_key || trigger.is_some_and(|trigger| self.gamepads.holding(trigger)));
        if fast {
            // the output waits out fast forward rather than run dry
            if let Some(audio) = &self.audio {
                audio.hold();
            }
        }
        if fast != self.fast_forwarding {
            self.fast_forwarding = fast;
            if !fast {
                // back at the console's pace, with no sound piled up
                if let Some(audio) = &self.audio {
                    audio.clear();
                }
                self.next_frame = Instant::now();
                self.apply_audio_speed();
            }
        }
        let emulate_due = playing && (fast || now >= self.next_frame);
        let mut emulated_at = None;
        if emulate_due {
            let start = Instant::now();
            emulated_at = Some(start);
            let started = timing::ticks();
            let due = self.next_frame;
            self.emulate();
            self.spent.emulating.add(start.elapsed());
            if let (Some(timing), Some(game)) = (&mut self.timing, &self.game) {
                timing.emulated(started, due, game.frame);
            }
            if fast {
                self.next_frame = Instant::now();
                self.pacing.clear();
            } else {
                self.next_frame += FRAME_TIME;
                if let Some((vblank, refresh)) = timing::vblank() {
                    self.next_frame = self.pacing.align(self.next_frame, FRAME_TIME, vblank, refresh);
                }
            }
        } else if !playing {
            self.next_frame = now;
            if let Some(audio) = &self.audio {
                // the sound stops with the game, which is not falling behind
                audio.hold();
            }
        }

        // fast forward shows at the console's rate whatever it runs at
        let cadence = if fast { FRAME_TIME } else { self.active.fps_limit.presentation_time().unwrap_or(self.refresh) };

        let after = Instant::now();
        let present_due = if fast {
            after >= self.next_shown
        } else {
            shows_now(emulate_due, playing, after, self.next_shown, self.next_frame, cadence)
        };

        if present_due {
            let start = Instant::now();
            let started = timing::ticks();
            self.present(event_loop);
            if let (Some(timing), Some(game)) = (&mut self.timing, &self.game) {
                timing.presented(started, game.frame);
            }
            if let Some(emulated) = emulated_at.filter(|_| !fast) {
                self.pacing.record(emulated.elapsed());
            }
            // counted from this present, so the ones in between a new
            // frame's fall evenly between frames
            self.next_shown = start + cadence;
            self.spent.showing.add(start.elapsed());
            self.shown += 1;
        }

        if self.last_title_update.elapsed() >= Duration::from_millis(500) {
            let elapsed_secs = self.last_title_update.elapsed().as_secs_f32();
            let shown = self.shown as f32 / elapsed_secs;
            self.shown_fps = shown;
            self.shown = 0;
            self.last_title_update = Instant::now();
            let refresh = timing::refresh().unwrap_or(UNKNOWN_REFRESH);
            if refresh != self.refresh {
                self.refresh = refresh;
                if !self.fast_forwarding {
                    self.apply_audio_speed();
                }
            }
            if let Some(game) = &self.game {
                let Spent { emulating, showing } = std::mem::take(&mut self.spent);
                log::debug!(
                    target: "zakuro::fps",
                    "{:.1} frames emulated and {shown:.1} shown a second, emulating took {emulating}, showing {showing}",
                    game.fps
                );
            }
            if let Some(window) = &self.window {
                let title = match &self.game {
                    Some(game) => format!("Zakuro - {} - {}", game.name, game.system.status_line()),
                    None => "Zakuro".to_owned(),
                };
                window.set_title(&title);
            }
            let underruns = self.audio.as_ref().map_or(0, |audio| audio.take_underruns());
            if underruns > 0 && playing {
                log::warn!("the sound ran dry {underruns} times, the emulation is falling behind");
            }
        }

        if !fast {
            let sleep = sleep_for(playing, Instant::now(), self.next_shown, self.next_frame, cadence);
            if sleep > Duration::from_millis(1) {
                std::thread::sleep(sleep - Duration::from_micros(500));
            }
        }
    }

    /// runs one frame of the game.
    fn emulate(&mut self) {
        let Some(game) = &mut self.game else { return };
        let input = match &mut game.replay {
            Some(replay) => replay.input(game.frame),
            None => self.gamepads.apply(self.keyboard.state()),
        };
        game.frame += 1;
        if let Some(recorder) = &mut game.recorder {
            recorder.record(input);
        }
        game.system.set_input(input);
        let outcome = game.system.run_frame();
        // the overlay reads the game the program was built on, not another
        // one its library opened
        let ours = self.linked.as_ref().is_none_or(|linked| game.system.title.as_ref().is_some_and(|title| title.program_id() == linked.program_id()));
        if let Some(overlay) = self.overlay.as_mut().filter(|_| ours) {
            let text = overlay.frame(&mut game.system.memory);
            self.overlay_corner = text.corner;
            if let Some(notice) = text.notice {
                self.menus.toast = Some((notice, Instant::now()));
            }
        }
        game.count_frame();
        let sound = game.system.take_audio();
        // fast forward plays no sound, there is too much of it too soon
        if let (Some(audio), false) = (&self.audio, self.fast_forwarding) {
            let heard = audio.push(&sound);
            if let (Some(timing), Some(heard)) = (&mut self.timing, heard) {
                timing.heard(heard, game.frame);
            }
        }
        match outcome {
            FrameOutcome::Completed => {}
            FrameOutcome::Exited => {
                log::info!("the title exited");
                self.back_to_library();
            }
            FrameOutcome::Faulted => {
                log::error!("the title stopped on a fault");
                let errors = game.system.fatal_errors.join("; ");
                if !errors.is_empty() {
                    log::error!("{errors}");
                }
                let message = format!("The game stopped on a fault. {errors}");
                self.menus.report = Some((message.clone(), report::text(&game.name, &game.system)));
                self.menus.message = Some(message);
                self.back_to_library();
            }
        }
    }

    /// the name the player goes by, the console's too.
    fn nickname(&self) -> String {
        match self.settings.nickname.trim() {
            "" => "Player".into(),
            name => name.chars().take(10).collect(),
        }
    }

    /// gives the running game the room the player is in, and tells the room
    /// the game.
    fn link_room(&mut self) {
        let Some(game) = &mut self.game else { return };
        game.system.room = self.room.as_ref().map(|room| room.link());
        if let Some(room) = &self.room {
            room.playing(&game.name);
        }
    }

    /// opens a room on the port chosen, and joins it.
    fn host_room(&mut self) {
        let name = match self.settings.room_name.trim() {
            "" => format!("{}'s room", self.nickname()),
            name => name.to_owned(),
        };
        let password = self.menus.room_password.clone();
        match zakuro_core::room::Server::start(self.settings.room_port, &name, &password, 16) {
            Ok(server) => {
                let address = format!("127.0.0.1:{}", server.port());
                self.room_server = Some(server);
                self.join_room(&address);
                if self.room.is_none() {
                    self.room_server = None;
                }
            }
            Err(error) => self.menus.message = Some(format!("The room could not be opened on port {}, {error}", self.settings.room_port)),
        }
    }

    /// joins the room at address.
    fn join_room(&mut self, address: &str) {
        let password = self.menus.room_password.clone();
        match zakuro_core::room::Connection::join(address.trim(), &self.nickname(), &password) {
            Ok(room) => {
                self.room = Some(room);
                self.link_room();
            }
            Err(error) => self.menus.message = Some(format!("Could not join the room, {error}.")),
        }
    }

    /// leaves the room, and closes it when it is this program's.
    fn leave_room(&mut self) {
        self.dlp_guest = None;
        self.room = None;
        self.room_server = None;
        if let Some(game) = &mut self.game {
            game.system.room = None;
        }
    }

    /// Download Play as a guest: hears the offers in the room while no game
    /// runs, and starts the child once the host's game begins.
    fn download_play(&mut self) {
        let Some(room) = self.room.as_ref().filter(|_| self.game.is_none()) else {
            self.dlp_guest = None;
            return;
        };
        let guest = self.dlp_guest.get_or_insert_with(|| zakuro_core::download_play::Guest::new(room.link()));
        guest.poll();
        match guest.end() {
            Some(zakuro_core::download_play::Phase::Begun { .. }) if let Some(name) = recompile::blocking_play(&self.jobs, &self.waiting) => {
                // like any other start, this one waits for the recompile
                self.dlp_own = None;
                self.menus.message = Some(format!("Download Play can start once {name} finishes recompiling, which takes the whole processor."));
            }
            Some(zakuro_core::download_play::Phase::Begun { child, reboot }) => {
                let own = self.dlp_own.take();
                let path = match child {
                    Some(child) => self.keep_child(&child),
                    None => own.ok_or_else(|| "the game to download went missing".to_owned()),
                };
                let started = path.and_then(|path| {
                    self.wireless_reboot = Some(reboot);
                    self.play(&path)
                });
                if let Err(error) = started {
                    self.wireless_reboot = None;
                    self.menus.message = Some(format!("Download Play could not start the game, {error}."));
                }
            }
            Some(zakuro_core::download_play::Phase::Failed(why)) => {
                self.dlp_own = None;
                self.menus.message = Some(format!("Download Play stopped, {why}."));
            }
            _ => {}
        }
    }

    /// takes part in the Download Play offer at index, with the player's own
    /// copy of the child when the folder has one.
    fn join_download_play(&mut self, index: usize) {
        if let Some(name) = recompile::blocking_play(&self.jobs, &self.waiting) {
            self.menus.message = Some(format!("Download Play can start once {name} finishes recompiling, which takes the whole processor."));
            return;
        }
        let node = zakuro_core::download_play::node(self.settings.console_id, &self.nickname());
        let Some(guest) = &mut self.dlp_guest else { return };
        let Some(heard) = guest.offers().get(index) else { return };
        let (host, child) = (heard.host, heard.offer.child);
        self.dlp_own = self.library.children.iter().find(|own| own.title_id == child).map(|own| own.path.clone());
        guest.join(host, node, self.dlp_own.is_some());
    }

    /// keeps a child the host sent, for it to run from.
    fn keep_child(&self, child: &[u8]) -> Result<PathBuf, String> {
        let cia = zakuro_fs::Cia::read(child).map_err(|_| "the host sent something that isn't a game".to_owned())?;
        let folder = self.data_dir.clone().unwrap_or_else(std::env::temp_dir).join("dlp");
        let path = folder.join(format!("{:016X}.cia", cia.title_id));
        std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(&path, child)).map_err(|error| error.to_string())?;
        Ok(path)
    }

    /// the file of a slot of the running game's saved states.
    fn state_path(&self, slot: u8) -> Option<PathBuf> {
        let program_id = self.game.as_ref()?.system.title.as_ref()?.program_id();
        Some(self.data_dir.as_ref()?.join("states").join(format!("{program_id:016X}")).join(format!("{slot}.state")))
    }

    /// saves the running game's state to the slot chosen.
    fn save_state(&mut self) {
        let slot = self.menus.slot;
        let Some(path) = self.state_path(slot) else { return };
        let Some(game) = &mut self.game else { return };
        let started = Instant::now();
        let saved = game.system.save_state().and_then(|bytes| {
            let parent = path.parent().expect("a state's file is in a folder");
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            // written beside it first, a state cut short never takes the
            // place of a whole one
            let partial = path.with_extension("state.new");
            std::fs::write(&partial, &bytes).and_then(|()| std::fs::rename(&partial, &path)).map_err(|error| error.to_string())
        });
        match saved {
            Ok(()) => {
                log::info!("saved the state to {} in {:?}", path.display(), started.elapsed());
                self.menus.toast = Some((format!("Saved to slot {slot}"), Instant::now()));
            }
            Err(error) => self.menus.message = Some(format!("The state could not be saved, {error}")),
        }
        // the time it took is not the game's to make up
        self.next_frame = Instant::now();
    }

    /// loads the running game's state from the slot chosen.
    fn load_state(&mut self) {
        let slot = self.menus.slot;
        let Some(path) = self.state_path(slot) else { return };
        let Some(game) = &mut self.game else { return };
        let loaded = std::fs::read(&path)
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => format!("Slot {slot} has no state saved."),
                _ => format!("The state could not be read, {error}"),
            })
            .and_then(|bytes| game.system.load_state(&bytes).map_err(|error| format!("The state could not be loaded, {error}.")));
        match loaded {
            Ok(()) => {
                log::info!("loaded the state from {}", path.display());
                self.menus.toast = Some((format!("Loaded slot {slot}"), Instant::now()));
                self.menus.menu_open = false;
                // the overlay's line was about the memory before, the next
                // frame gives the one about the state
                self.overlay_corner = None;
                if let Some(audio) = &self.audio {
                    audio.clear();
                }
            }
            Err(error) => self.menus.message = Some(error),
        }
        self.next_frame = Instant::now();
    }

    /// the file of the running game's cheats.
    fn cheats_path(&self) -> Option<PathBuf> {
        let program_id = self.game.as_ref()?.system.title.as_ref()?.program_id();
        Some(zakuro_core::cheats::path(self.data_dir.as_ref()?, program_id))
    }

    /// writes the running game's cheats to their file.
    fn save_cheats(&mut self) {
        let (Some(path), Some(game)) = (self.cheats_path(), &self.game) else { return };
        if let Err(error) = zakuro_core::cheats::save(&path, &game.system.cheats) {
            self.menus.message = Some(format!("The cheats can't be saved: {error}"));
        }
    }

    /// the menus, and the actions they asked for, done.
    fn interface(&mut self, event_loop: &ActiveEventLoop) -> Overlay {
        self.poll_store();
        self.ask_about_added();
        // when the slot chosen was saved to, while the menu shows it
        let saved = self
            .menus
            .menu_open
            .then(|| self.state_path(self.menus.slot))
            .flatten()
            .and_then(|path| std::fs::metadata(path).ok()?.modified().ok())
            .map(ago);
        let program_id = self.program_id();
        let (Some(gui), Some(window)) = (&mut self.gui, &self.window) else { return Overlay::default() };
        // the cursor goes while buttons drive the interface, and comes back
        // when the mouse moves
        let hide = self.menus.mode == InputMode::Directional && (self.game.is_none() || self.menus.over_game());
        if hide != self.cursor_hidden {
            window.set_cursor_visible(!hide);
            self.cursor_hidden = hide;
        }
        let keyboard = self.game.as_ref().and_then(|game| game.system.keyboard_request().cloned());
        let room = self.room.as_ref().filter(|_| self.menus.multiplayer_open).map(|connection| {
            let members = connection.members();
            menus::RoomView {
                name: connection.room().to_owned(),
                download_play: self.dlp_guest.as_ref().map(|guest| download_play_view(guest, &members, &self.library)),
                members: members.into_iter().map(|member| (member.nickname, member.game)).collect(),
                port: self.room_server.as_ref().map(|server| server.port()),
            }
        });
        let cheats: Vec<menus::CheatView> = self
            .game
            .as_ref()
            .filter(|_| self.menus.cheats_open)
            .map(|game| {
                game.system
                    .cheats
                    .iter()
                    .map(|cheat| menus::CheatView { name: cheat.name.clone(), enabled: cheat.enabled, notes: cheat.notes.join("\n"), builtin: cheat.builtin })
                    .collect()
            })
            .unwrap_or_default();
        let running = self.game.as_ref().map(|game| menus::RunningView {
            name: &game.name,
            program_id,
            recompiled: game.system.recompiled.is_some(),
            saved: saved.as_deref(),
            fps: self.active.show_fps.then_some(self.shown_fps),
            fast: self.fast_forwarding,
            started: Some(&self.started),
        });
        let view = menus::View {
            library: &self.library,
            jobs: &self.jobs,
            waiting: &self.waiting,
            running,
            cheats: &cheats,
            install: self.install.as_ref().map(store::Install::progress),
            identifying: self.identifying.is_some(),
            room: room.as_ref(),
            keyboard: keyboard.as_ref(),
            corner: self.overlay_corner.as_deref(),
            data_dir: self.data_dir.as_deref(),
        };
        let (menus, settings) = (&mut self.menus, &mut self.settings);
        let modal_open = menus.modal_open();
        gui.set_theme(window, settings.theme);
        let mut actions = Vec::new();
        let overlay = gui.frame(window, |ui| actions.extend(menus.show(ui, settings, &view)));
        drop(view);
        // a window closed over a game lets go of what was held to close it
        if modal_open && !self.menus.modal_open() {
            self.release_input();
        }
        self.library.changed = false;
        for action in actions {
            self.act(action, event_loop);
        }
        overlay
    }

    fn present(&mut self, event_loop: &ActiveEventLoop) {
        let overlay = self.interface(event_loop);
        let blank = |screen: Screen| ((std::sync::Arc::new(Vec::new()), screen.width(), screen.height()), None);
        // the right eye's picture only while the 3D shows
        let stereo = self.active.stereo != settings::Stereo3d::Off;
        let (top, bottom, right) = match &mut self.game {
            Some(game) => (
                shown(&mut game.system, Screen::Top),
                shown(&mut game.system, Screen::Bottom),
                stereo.then(|| shown_right(&mut game.system)).flatten(),
            ),
            None => (blank(Screen::Top), blank(Screen::Bottom), None),
        };
        let right = right.unwrap_or_else(|| blank(Screen::Top));
        let Some(backend) = &mut self.backend else { return };
        let (((top, top_width, top_height), top_gpu), ((bottom, bottom_width, bottom_height), bottom_gpu)) = (top, bottom);
        let ((right, right_width, right_height), right_gpu) = right;
        let result = backend.present(
            ScreenImage { width: top_width, height: top_height, pixels: &top, gpu: top_gpu },
            ScreenImage { width: bottom_width, height: bottom_height, pixels: &bottom, gpu: bottom_gpu },
            ScreenImage { width: right_width, height: right_height, pixels: &right, gpu: right_gpu },
            &overlay,
        );
        match result {
            Ok(()) | Err(PresentError::OutOfDate) => {}
            Err(error) => {
                log::error!("presentation failed: {error}");
                event_loop.exit();
            }
        }
    }
}

/// what the multiplayer window shows of Download Play: the offers in the
/// room, and where the guest is.
fn download_play_view(guest: &zakuro_core::download_play::Guest, members: &[zakuro_core::room::Member], library: &Library) -> menus::DlpView {
    use zakuro_core::download_play::Phase;
    let offers = guest
        .offers()
        .iter()
        .map(|heard| {
            let offer = &heard.offer;
            let smdh = library::Smdh::parse(&offer.smdh);
            let own = library.children.iter().any(|own| own.title_id == offer.child);
            let blocked = if offer.guests >= offer.most {
                Some("The host's game takes no more guests.".to_owned())
            } else if !offer.sends_child && !own {
                Some(
                    "The game to download is encrypted on the card, so either player needs a decrypted copy of it: \
                     decrypt the .cia in the card's Download Play partition with GodMode9 and put it in the games folder."
                        .to_owned(),
                )
            } else {
                None
            };
            menus::OfferView {
                child: offer.child,
                title: smdh.as_ref().map(|smdh| smdh.name.clone()).filter(|name| !name.is_empty()).unwrap_or_else(|| format!("{:016X}", offer.child)),
                host: members.iter().find(|member| member.mac == heard.host).map(|member| member.nickname.clone()).unwrap_or_default(),
                icon: smdh.map(|smdh| smdh.icon),
                guests: offer.guests,
                most: offer.most,
                blocked,
            }
        })
        .collect();
    let doing = guest.phase().map(|phase| match phase {
        Phase::Asking => ("Asking the host to join.".to_owned(), None),
        Phase::Waiting => ("Joined. The game downloads once the host starts it.".to_owned(), None),
        Phase::Receiving { got, total } => ("Downloading the game.".to_owned(), Some(*got as f32 / (*total).max(1) as f32)),
        Phase::Ready => ("Ready. Waiting for the host to start the game.".to_owned(), None),
        Phase::Begun { .. } => ("Starting the game.".to_owned(), None),
        Phase::Failed(why) => (why.clone(), None),
    });
    menus::DlpView { offers, doing }
}

/// a screen as the presenter takes it, straight from the GPU when it shares
/// the renderer's device and the GPU drew what the screen shows, else its
/// pixels.
fn shown(system: &mut System, screen: Screen) -> (zakuro_core::Screen, Option<GpuScreen>) {
    if let Some(gpu) = system.gpu_screen(screen) {
        let scale = system.gpu.scale();
        return ((std::sync::Arc::new(Vec::new()), screen.width() * scale, screen.height() * scale), Some(gpu));
    }
    (system.read_screen_scaled(screen), None)
}

/// the top screen's picture for the right eye, as shown gives the screens,
/// when the game draws in 3D.
fn shown_right(system: &mut System) -> Option<(zakuro_core::Screen, Option<GpuScreen>)> {
    if let Some(gpu) = system.gpu_right_eye() {
        let scale = system.gpu.scale();
        return Some(((std::sync::Arc::new(Vec::new()), Screen::Top.width() * scale, Screen::Top.height() * scale), Some(gpu)));
    }
    system.read_right_eye_scaled().map(|screen| (screen, None))
}

/// keeps the renderer kind referenced even when a backend feature is off.
const _: RendererKind = RendererKind::Software;

#[cfg(test)]
mod tests {
    use super::*;

    /// a new frame shows at once, and the pictures in between fall halfway
    /// between frames, never just before one.
    #[test]
    fn frames_show_as_soon_as_they_are_emulated() {
        let t = Instant::now();
        let at = |ms: f64| t + Duration::from_secs_f64(ms / 1000.0);
        let fps = |n: u64| Duration::from_nanos(1_000_000_000 / n);
        // 60: every frame, nothing in between, though the limit's tick,
        // 16.67 ms, comes before the console's next frame, 16.71 ms
        assert!(shows_now(true, true, at(16.71), at(16.67), at(33.4), fps(60)));
        assert!(shows_now(true, true, at(10.0), at(16.67), at(26.71), fps(60)), "even when the limit's clock says early");
        assert!(!shows_now(false, true, at(16.68), at(16.67), at(16.71), fps(60)));
        // 120: the frame at once, and one more halfway to the next
        assert!(shows_now(true, true, at(12.0), at(16.67), at(28.71), fps(120)), "a late frame right after one in between");
        assert!(shows_now(false, true, at(8.4), at(8.33), at(16.71), fps(120)));
        assert!(!shows_now(false, true, at(8.2), at(8.33), at(16.71), fps(120)), "not before its time");
        assert!(!shows_now(false, true, at(14.0), at(8.33), at(16.71), fps(120)), "not just before the next frame");
        // 30: every other frame
        assert!(!shows_now(true, true, at(16.71), at(33.33), at(33.42), fps(30)));
        assert!(shows_now(true, true, at(33.42), at(33.33), at(50.13), fps(30)));
        // menus over a paused game keep to the limit
        assert!(shows_now(false, false, at(16.7), at(16.67), at(0.0), fps(60)));
    }

    #[test]
    fn steps_sleep_until_there_is_something_to_do() {
        let t = Instant::now();
        let at = |ms: f64| t + Duration::from_secs_f64(ms / 1000.0);
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let fps = |n: u64| Duration::from_nanos(1_000_000_000 / n);
        // 60: the limit's tick passed, but nothing shows until the next frame
        assert!((ms(sleep_for(true, at(1.0), at(0.5), at(16.71), fps(60))) - 15.71).abs() < 0.01);
        // 120: wakes for the picture halfway
        assert!((ms(sleep_for(true, at(1.0), at(8.33), at(16.71), fps(120))) - 7.33).abs() < 0.01);
        // but not for one that would come just before the frame
        assert!((ms(sleep_for(true, at(1.0), at(14.0), at(16.71), fps(120))) - 15.71).abs() < 0.01);
        // paused: the menus' own pace
        assert!((ms(sleep_for(false, at(1.0), at(16.67), at(0.0), fps(60))) - 15.67).abs() < 0.01);
    }
}
