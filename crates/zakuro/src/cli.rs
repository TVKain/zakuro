//! command line parsing.

use zakuro_gpu::RendererKind;

#[derive(Debug, Clone)]
pub struct Options {
    /// the game to start, none to open the library.
    pub rom: Option<String>,
    /// the settings' choice when not given.
    pub renderer: Option<RendererKind>,
    /// window scale relative to the console's 400x480 combined screens, the
    /// settings' when not given.
    pub scale: Option<u32>,
    /// run without a window for this many frames, then report.
    pub headless: Option<u64>,
    pub profile: bool,
    pub new3ds: bool,
    /// skip loading a ROM entirely and just paint both screens solid colors.
    pub test_pattern: bool,
    /// a library of recompiled code for the title, or a directory of them.
    pub recompiled: Option<String>,
    /// draw on the host GPU rather than in software, the settings' choice
    /// when not given.
    pub hardware_rasterizer: Option<bool>,
    /// where saves live, instead of the usual place.
    pub data: Option<String>,
    /// interpret everything, whatever recompiled code there is.
    pub interpreter: bool,
    /// play no sound.
    pub mute: bool,
    /// log debug output and attach or allocate a console on Windows.
    pub debug: bool,
}

const USAGE: &str = "\
zakuro - a high-level-emulation Nintendo 3DS emulator

usage: zakuro [rom.3ds|.cxi] [options]

Without a game it opens the library, the games in a folder you pick. The
options below override the settings for this run.

options:
  --renderer <vulkan|gl|software>  presentation backend (default: vulkan,
                                   or gl where Vulkan does not start)
  --rasterizer <hardware|software> where the 3D is drawn (default: hardware,
                                   the host GPU through Vulkan, or software
                                   where that does not start)
  --scale <n>                      window scale factor (default: 2)
  (Esc opens the menu over a game, F1 pauses, F11 goes fullscreen)
  --headless <frames>              run without a window and print a report
  --new3ds                         emulate a New 3DS
  --profile                        collect a sampling profile and print it
  --recompiled <path>              run code 3dsrecomp built for the title, a
                                   library or a directory holding <title id>.so
                                   (.dylib on macOS, .dll on Windows, default:
                                   the one 3dsrecomp build installed for it, in
                                   the system's place for data,
                                   ~/.local/share/3dsrecomp,
                                   ~/Library/Application Support/3dsrecomp or
                                   %APPDATA%\\3dsrecomp)
  --interpreter                    interpret everything, even with recompiled
                                   code around
  --mute                           play no sound
  -d, --debug                      log debug output and attach or allocate a console on Windows
  --data <dir>                     where saves live (default: the system's
                                   place for data, ~/.local/share/zakuro,
                                   ~/Library/Application Support/zakuro or
                                   %APPDATA%\\zakuro)
  -h, --help                       show this message

The system font cannot be generated: put a dump at sysdata/shared_font.bin, in
the working directory or where saves live, for titles that render text with it.
";

#[cfg(windows)]
pub fn attach_console() {
    extern "system" {
        fn AttachConsole(dwProcessId: u32) -> i32;
        fn AllocConsole() -> i32;
        fn SetStdHandle(nStdHandle: u32, hHandle: isize) -> i32;
        fn CreateFileW(
            lpFileName: *const u16,
            dwDesiredAccess: u32,
            dwShareMode: u32,
            lpSecurityAttributes: *const std::ffi::c_void,
            dwCreationDisposition: u32,
            dwFlagsAndAttributes: u32,
            hTemplateFile: isize,
        ) -> isize;
    }
    const ATTACH_PARENT_PROCESS: u32 = 0xFFFF_FFFF;
    const STD_OUTPUT_HANDLE: u32 = 0xFFFF_FFF5;
    const STD_ERROR_HANDLE: u32 = 0xFFFF_FFF4;
    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;

    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 || AllocConsole() != 0 {
            let conout: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
            let handle = CreateFileW(
                conout.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                0,
            );
            if handle != -1 {
                SetStdHandle(STD_OUTPUT_HANDLE, handle);
                SetStdHandle(STD_ERROR_HANDLE, handle);
            }
        }
    }
}

pub fn print_console(text: &str) {
    #[cfg(windows)]
    attach_console();
    print!("{text}");
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

pub fn parse() -> Result<Options, String> {
    let mut rom = None;
    let mut options = Options {
        rom: None,
        renderer: None,
        scale: None,
        headless: None,
        profile: false,
        new3ds: false,
        test_pattern: false,
        recompiled: None,
        hardware_rasterizer: None,
        data: None,
        interpreter: false,
        mute: false,
        debug: false,
    };

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_console(USAGE);
                std::process::exit(0);
            }
            "-d" | "--debug" => options.debug = true,
            "--renderer" => {
                let value = args.next().ok_or("--renderer needs a value")?;
                options.renderer = Some(
                    RendererKind::parse(&value).ok_or_else(|| format!("unknown renderer '{value}'"))?,
                );
            }
            "--scale" => {
                let value = args.next().ok_or("--scale needs a value")?;
                options.scale = Some(value.parse().map_err(|_| format!("'{value}' is not a scale"))?);
            }
            "--headless" => {
                let value = args.next().ok_or("--headless needs a frame count")?;
                options.headless = Some(
                    value
                        .parse()
                        .map_err(|_| format!("'{value}' is not a frame count"))?,
                );
            }
            "--new3ds" => options.new3ds = true,
            "--rasterizer" => {
                let value = args.next().ok_or("--rasterizer needs a value")?;
                options.hardware_rasterizer = Some(match value.as_str() {
                    "hardware" | "vulkan" | "gpu" => true,
                    "software" | "cpu" => false,
                    other => return Err(format!("unknown rasterizer '{other}'")),
                });
            }
            "--recompiled" => {
                options.recompiled = Some(args.next().ok_or("--recompiled needs a path")?);
            }
            "--data" => {
                options.data = Some(args.next().ok_or("--data needs a directory")?);
            }
            "--interpreter" => options.interpreter = true,
            "--mute" => options.mute = true,
            "--profile" => options.profile = true,
            "--test-pattern" => options.test_pattern = true,
            // macOS hands an app it hasn't run before a process number
            other if cfg!(target_os = "macos") && other.starts_with("-psn_") => {}
            other if other.starts_with('-') => {
                return Err(format!("unknown option '{other}'"));
            }
            path => rom = Some(path.to_owned()),
        }
    }

    options.rom = rom;
    if options.headless.is_some() && options.rom.is_none() && !options.test_pattern {
        print_console(USAGE);
        return Err("--headless needs a game".to_owned());
    }
    Ok(options)
}
