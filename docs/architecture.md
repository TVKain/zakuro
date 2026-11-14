# Architecture

## Repository layout

| Path | What it is |
| :--- | :--- |
| `crates/zakuro-common` | Types shared by every crate: the 3DS memory map, result codes, bit helpers |
| `crates/zakuro-fs` | ROM containers: NCSD (`.3ds`), CIA, NCCH, ExeFS, RomFS, BLZ decompression, IPS/BPS patches, mod layering over RomFS |
| `crates/zakuro-cpu` | The ARM11 (ARMv6K) interpreter: ARM, Thumb, VFPv2, CP15 |
| `crates/zakuro-gpu` | The PICA200: registers, shader units, the software and Vulkan rasterizers, texture packs, frame interpolation, and the presenters that put the screens in a window |
| `crates/zakuro-core` | The console: memory, the HLE kernel, HLE services, the loader, recompiled-code runner, saved states, cheats, mods, local wireless |
| `crates/zakuro` | The frontend program: window, event loop, menus, input, audio output, frame pacing, library, recompiling |
| `crates/zakuro-setup` | The Windows installer and uninstaller |
| `vendor/3dsrecomp` | The ahead-of-time recompiler, vendored with local patches, outside the workspace ([recompiling.md](recompiling.md)) |
| `packaging/` | Icons, the Windows resource script, Linux and macOS bundle scripts, `package-windows.ps1`, `release.ps1` |
| `tools/` | Test pipeline and dev tools ([testing.md](testing.md)) |
| `.github/` | CI (build and test on Windows, Linux, macOS), release workflow, issue template |

The dependency order is `common` → `fs`, `cpu` → `gpu` → `core` → `zakuro`.
The lower crates know nothing about windows, audio devices or settings files.
Keep it that way: the frontend passes things down through `zakuro_core::Config`
and `System` methods.

## One frame, from game code to the screen

The frontend's `App::step` (`crates/zakuro/src/lib.rs`) runs on every pass of
winit's event loop:

1. **Poll.** Library scans, install and recompile jobs, gamepads, local
   wireless rooms and Download Play.
2. **Emulate** when the next console frame is due (`next_frame`, 59.83 Hz,
   nudged by `pacing.rs` to line up with the display's vblank).
   `App::emulate` reads input (keyboard and gamepads, or a replay), calls
   `System::set_input` and `System::run_frame`, then hands the frame's sound to
   `audio.rs`.
3. **Present** when `shows_now` says so (usually right after a new frame).
   `App::present` reads the screens from the core and calls
   `Backend::present` (`present.rs`) with the egui overlay.
4. **Sleep** until the next frame or present (`sleep_for`).

Inside `System::run_frame` (`crates/zakuro-core/src/lib.rs`):

- **CPU.** The guest's threads run until the frame's cycle budget (4,481,136
  cycles) is spent. Code runs from the recompiled library
  (`recompiled.rs`) when there is one and it covers the address. Otherwise it
  goes to the interpreter in `zakuro-cpu`. `idle.rs` detects spin loops and
  skips them.
- **Kernel.** `kernel/svc.rs` dispatches supervisor calls: threads, sync
  objects, memory, IPC. `kernel/mod.rs` schedules threads.
- **Services.** IPC requests go to the HLE services in `services/`: `fs`,
  `gsp` (GPU), `dsp` (audio), `hid` (input), `apt`, `cfg`, `uds` (local
  wireless), `cam`, `mic`, `y2r`, and more. `services/mod.rs` routes them.
- **GPU.** `gsp` forwards command lists, memory fills and display transfers to
  `zakuro-gpu`. Draw calls run vertex shaders on the CPU (interpreter or the
  Cranelift JIT, `shader/jit.rs`) or on the GPU (`shade.vert`). Triangles
  are filled by the software rasterizer (`raster.rs`) or the Vulkan one
  (`raster/hardware.rs`).
- **Audio.** The DSP service mixes the game's voices (`dsp_voices.rs`,
  `dsp_mixer.rs`) at 32,728 Hz. The frontend collects them with
  `System::take_audio`.

The presenters (`zakuro-gpu/src/backend/`) scale the screens into the window
with the chosen display filter. When the renderer and the Vulkan presenter
share a device (`device.rs`), screens are shown straight from GPU memory.
Otherwise they're read back and uploaded. See [rendering.md](rendering.md).

## Frontend modules (`crates/zakuro/src`)

| Module | Role |
| :--- | :--- |
| `lib.rs` | `App`: window, event loop, the frame loop above, actions, saved states, rooms |
| `menus.rs` | All the UI in egui: Home, System Settings, game Options, pause menu, pickers, prompts, theme palette (`color`) |
| `gui.rs` | egui integration: feeds window events in, gives the overlay to the presenter, sets the theme |
| `nav.rs` | One navigation vocabulary for keyboard, mouse, touch and controller |
| `input.rs`, `gamepad.rs`, `pro_controller.rs` | Keyboard mapping, gilrs controllers, the Switch Pro Controller on macOS |
| `settings.rs` | `Settings` and per-game `GameSettings`, saved as `settings.toml`, with migrations |
| `library.rs`, `store.rs` | Scanning the library, and adding games, updates, DLC, texture packs and cheats to it |
| `recompile.rs`, `zig.rs` | Running 3dsrecomp on a background thread, finding a C compiler |
| `present.rs` | Creating the window with a Vulkan or OpenGL presenter |
| `audio.rs` | Output through cpal: queue, time-stretcher, resampler |
| `pacing.rs`, `timing.rs` | Locking frames to the display; the `ZAKURO_TIMING` record |
| `camera.rs`, `microphone.rs` | Host camera and microphone for the console's |
| `icons.rs` | Outline icons as SVG paths, rasterized at the screen's pixel size |
| `cli.rs`, `report.rs` | Command-line parsing; the log file and fault reports |

## Where to look for…

| Task | Start in |
| :--- | :--- |
| A game crashes or hangs | The log (`zakuro.log`), then `kernel/svc.rs` and the service it was talking to; `examples/trace.rs` and `watch.rs` |
| Wrong colours, missing geometry | `zakuro-gpu/src/raster/hardware.rs` and `shaders/raster.frag`; compare with `--rasterizer software` |
| Scaling, filters, sharpness | `shaders/present_common.glsl`, `backend/vulkan.rs`, `backend/gl.rs` |
| Stutter, input lag | `crates/zakuro/src/lib.rs` (`step`, `shows_now`), `pacing.rs`, measured with `ZAKURO_TIMING` |
| Crackles, sync, pitch | `crates/zakuro/src/audio.rs`, `services/dsp_voices.rs` |
| A setting | `settings.rs` (data), `menus.rs` (`setting()`, `page_entries()`), `App::apply_settings` |
| Menu navigation | `nav.rs`, `menus.rs` (`handle`, `Op`) |
| A file format | `zakuro-fs` |
| Recompiled code misbehaves | `zakuro-core/src/recompiled.rs`, `hints.rs`, `vendor/3dsrecomp` |

## Environment variables

These change behaviour for debugging and testing.

The program (`zakuro`):

| Variable | Effect |
| :--- | :--- |
| `ZAKURO_REPLAY=<file>` | Play scripted input from the first frame ([testing.md](testing.md#replays)) |
| `ZAKURO_RECORD=<file>` | Write a replay of the session |
| `ZAKURO_TIMING=<file>` | Record emulate, present and audio times ([timing-and-audio.md](timing-and-audio.md)) |
| `ZAKURO_SHADER_JIT=0` | Interpret vertex shaders instead of JIT-compiling them (also applies to the examples) |
| `ZAKURO_PRESENT_MODE=fifo\|immediate\|mailbox\|…` | Force a Vulkan present mode |
| `RUST_LOG=<filter>` | Log filter, for example `zakuro=debug,zakuro::fps=debug` |

The program's own options (`--renderer vulkan|gl|software`,
`--rasterizer hardware|software`, `--recompiled <path>`, `--debug`) are listed
by `zakuro --help`.

The `boot` example (`crates/zakuro-core/examples/boot.rs`) takes the console's
configuration from the environment instead: `ZAKURO_RASTERIZER=hardware`,
`ZAKURO_RESOLUTION=<n>`, `ZAKURO_RECOMPILED=<path>`, `ZAKURO_FIND_RECOMPILED=1`,
`ZAKURO_UPDATE=<cia>`, `ZAKURO_DLC=<a.cia:b.cia>`, `ZAKURO_DATA=<dir>`,
`ZAKURO_NEW3DS=1`, `ZAKURO_NICK`, `ZAKURO_LANGUAGE`, `ZAKURO_PROFILE`,
`ZAKURO_CAMERA` and `ZAKURO_MIC`. Each one is explained in a comment where it's
read.

`grep -rn 'env::var' crates` finds the rest. Most are one-off debugging aids,
documented where they're read.
