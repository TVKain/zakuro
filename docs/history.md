# History

Bugs, root causes, measurements and decisions from this fork's work, oldest
first. The topic docs describe how things are now. This page explains why. When
an entry was replaced by later work, it says so. Add an entry when you fix
something a future contributor would otherwise have to rediscover.

Device names refer to [devices.md](devices.md).

## Cranelift JIT stack probing crash

**Symptom:** on the MSI Claw 8 AI+, Pokémon Ultra Sun crashed with
`0xC0000005` within about 60 frames of 3D rendering. The Lenovo LOQ (NVIDIA)
never crashed.

**Cause:** `shader/jit.rs` compiles PICA200 vertex shaders to host code with
Cranelift. Complex shaders got large stack frames, allocated in one
instruction (`sub $0xb4a0, %rsp`, 46,240 bytes, more than eleven 4 KB
pages). Windows grows a thread's stack one guard page at a time. With stack
probing off, the first write landed past the guard page, in uncommitted memory,
and caused an unhandled access violation. NVIDIA's driver had already pushed
64–128 KB of call frames on the same threads during Vulkan setup, so the
frames landed in committed memory and the bug stayed hidden. The Intel Arc
driver's threads had committed only their first page.

**Fix:** `("enable_probestack", "true")` and `("probestack_strategy",
"inline")` in the JIT's flags. Cranelift now touches every page in order
before moving `rsp`. Guarded by `hardware_architectural_regression.rs`.

## "Saved game data is corrupted"

**Cause:** an earlier crash left `user/savedata/<title>.format` without the
archive's `main`. `GetFormatInfo` saw `.format` and reported the archive as
formatted, opening `/main` failed, and the game decided its save was corrupt
rather than new.

**Fix (data):** deleting the incomplete folder and `.format` made
`GetFormatInfo` return `FS_NOT_FORMATTED`, and the game created a fresh save.
If you see this message on a first launch, check for an orphan `.format`.

## Handheld input

- **No Escape key on handhelds.** The controller's Menu/Home button opens the
  pause menu (configurable under Controls; the default is `Button::Mode`).
- **Touchscreen ignored in game.** egui handled `WindowEvent::Touch`, but the
  frontend only forwarded mouse events to the 3DS touchscreen. Touch events
  now drive it too (Started/Moved → down, Ended/Cancelled → up).
- **Button leaks.** Buttons held while closing a menu reached the game,
  hotkeys fired over dialogs, and backdrop clicks became touches. Fixed with
  input isolation while a menu is over the game, an Escape stack, and
  held-button suppression on resume ([frontend.md](frontend.md#isolation)).

## Presentation filters, first generation (replaced)

The first round added single-pass xBRZ-freescale (mode 3, with a BT.2020
colour metric and screen-bounds clamping), "xBRZ + Linear" (mode 4, 60/40
blend), 4-tap box supersampling with AMD CAS for downscaling, "smart vibrance"
on xBRZ, and cutoffs that switched methods at `covered` 1.001 and 1.5. A
later footprint-average pass replaced the 4-tap box.

All of that was replaced by the linear-light presenter below. CAS, vibrance,
the cutoffs and the separate GL copy of the filters are gone. Mode 3 remains
in the shader but isn't offered.

## Runtime resolution changes

Changing the internal resolution in a running game did nothing:
`apply_settings` never passed it on, and `Hardware::set_scale` refused while
surfaces existed. `set_scale` now ends rendering, waits for the device to go
idle, drops surfaces, copies, textures and samplers, and applies the new scale.
16x anisotropic filtering was enabled above 1x at the same time.

## GPU vertex shading on the Arc 140V

The Arc 140V reports `INTEGRATED_GPU`, and integrated GPUs ran vertex shaders
on the CPU. The rule now also accepts device names containing Arc, Iris,
GeForce or Radeon. Shading on the Arc takes about 48 µs per frame, and the CPU
is freed for the game.

## Frame rate limit decoupled from emulation

At first, the frame rate limit changed the console's speed: 120 FPS ran the
game at double speed and 30 FPS at half. Emulation is now locked to the
console's 59.83 Hz, and the limit only changes presentation. Frame skipping
(`skips_showing`) was removed. Later replaced in part by presenting at once
and the display lock (below).

## Windows GUI subsystem and icon

`zakuro.exe` opened a console window next to itself. It's now a GUI-subsystem
program (`windows_subsystem = "windows"`) that attaches a console only for
`--debug` or `--help`. The icon is compiled from `packaging/zakuro.rc` and
linked in. The pipeline's release step checks both.

## Regression pipeline and lint baseline

`crates/zakuro/tests/` got six integration suites, and `tools/test_regressions.ps1`
runs lint, tests, shader compilation and the release checks. Clippy with
`-D warnings` became the baseline (`--all-targets` since the dead-code pass).
Repeated UI and stick-flick code was deduplicated at the same time.

## Console-style UI rewrite

The first UI (tabs, GLOBAL/CUSTOM badges, two ~200-line input chains with
hand-numbered rows that drifted apart) was replaced by Home tiles, a shared
settings window, the `Entry` row model with one `handle(item, Op)`, `nav.rs`
for all input, and the managed library and store
([frontend.md](frontend.md)).

## Texture upscaling

At 8x, the game's own low-resolution textures still showed stair steps. "Upscale
textures" redraws them with alpha-aware xBRZ on the GPU (`scale.comp`), blended
35% with bilinear. Textures rewritten often (video) are skipped
([rendering.md](rendering.md#textures)).

## Bundled Zig and hidden compiler windows

Recompiling needs a C compiler. Instead of offering a download, the installer
puts Zig 0.17.0 next to `zakuro.exe`. Every `zig cc` the recompiler started
opened a console window, hundreds per recompile, so the vendored 3dsrecomp
starts them with `CREATE_NO_WINDOW` ([recompiling.md](recompiling.md#local-patches-to-the-vendored-3dsrecomp)).

## Mouse wheel in overlay windows

egui saw the overlay `Area`s as zero-size, because everything was drawn into
child UIs, so the wheel never reached their `ScrollArea`s. Each one now calls
`ui.expand_to_include_rect(screen)`. Clicks on a range row's `‹`/`›` use the
rects `paint_row` returns. They used to be guessed from the right edge, and
missed.

## Library only, one recompile at a time

Games come only from Zakuro's library. The user's games folder setting, the
`removed` list and `Library::hidden` are gone. Recompiles queue, and nothing
starts a game while one runs or waits. A game whose update or DLC changed
counts as outdated, through the `.addons` stamp. Prompts ask before
recompiling. The window always opens maximized, and the per-game window
settings are gone ([recompiling.md](recompiling.md), [frontend.md](frontend.md)).

## Installer

`zakuro-setup` replaced shipping Zig in the zip. It's a per-user install
that downloads Zig itself, plus an uninstaller that can delete its own
folder, with controller support ([releasing.md](releasing.md)).

## Graphics settings regroup, live changes, icons

The Graphics page was regrouped (Motion, Advanced). Every row got a help line,
and every picker choice a note. The four display filters became Sharp
(default), Soft, Pixelated and Smooth edges, with old names still loading.
The graphics API switches on Home without a restart, custom textures toggle
in game, and restart notices appear only when a value really changed. Outline
icons replaced text glyphs.

## Themes

Light, Dark and System themes, with a slate and violet palette, 4.5:1 text
contrast, and the Windows title bar following the theme. Home under a window
first paints a solid background. Without it, the black clear colour showed
through the half-opacity Home in Light ([frontend.md](frontend.md#themes)).

## Linear-light presenter and measured kernels

One filter source (`present_common.glsl`) for both presenters. Sampling goes
through sRGB views, so blending happens in linear light. Cubic kernels are
stretched over the footprint when shrinking, with paired bilinear taps. The
kernels were chosen by PSNR, SSIM and edge energy against an 8x Lanczos3
reference: Sharp at 4x went from 35.85 dB to 37.15 dB
([rendering.md](rendering.md#display-filters)).

## Faster recompiles

zig cc wrote CodeView debug info nobody used. `-g0` made a full Ultra Sun
build 31% faster (1055 s → 726 s) and its objects 4.5x smaller.

## Frame pacing locked to the display

A finished frame waited for the presenter's own clock, which added up to a
tick of lag and drifted against the console's 59.83 Hz. Frames are now
presented at once, and their start is nudged to finish 1.5 ms before a vblank
when the display's rate allows. At 60 FPS on 120 Hz, input → vblank went from
10.9 ms to 6.8 ms, and off-beat frames from 11% to 0.35%. Uncapped no longer
slows emulation ([timing-and-audio.md](timing-and-audio.md)).

## Audio latency and quality

Sound trailed the picture by 146 ms. The WSOLA stretcher held 15–20 ms even
when not stretching, and the queue target was 4096 samples. The stretcher now
passes sound through while idle, and the target is 1280 samples. Sound is now
44 ms behind, with no dropouts. Linear resampling to the device rate was
replaced with a 64-tap Kaiser sinc (64 dB → 104 dB at 440 Hz), and polyphase
voices play through a Catmull-Rom cubic instead of a straight line
([timing-and-audio.md](timing-and-audio.md#the-audio-path)).

## Dead code pass

Unused functions, types and placeholder constants were removed across the
crates, and clippy now covers `--all-targets`. `Cp15::icache_invalidated` and
`invalidate_ranges` stay because saved states depend on the field layout, but
nothing writes them anymore: the range list used to grow forever. Drop them at
the next version bump. Checked with every test, the pipeline, and a 140 s walk
(6.7 ms input → vblank, 0.56% off-beat, sound 41 ms after the picture).

## Developer docs move into the repository

The workspace-level guide, test pipeline and dev tools moved into the
repository: `AGENTS.md`, `docs/`, `tools/`. Release notes now live in
`docs/release-notes/` and packages in `target/dist`, so the repository no
longer depends on folders around it.
