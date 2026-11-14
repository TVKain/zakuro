# Testing

## The regression pipeline

`tools/test_regressions.ps1` (or `tools\test_regressions.bat` from cmd) is the
gate before every commit:

| Step | Checks |
| :--- | :--- |
| 1 | `cargo clippy --workspace --all-targets -- -D warnings` |
| 2 | `cargo test --workspace --lib`: unit tests in every crate |
| 3 | `cargo test -p zakuro --test "*"`: the frontend's integration suites |
| 4 | Every shader in `crates/zakuro-gpu/shaders` compiles, including the variants (needs `glslangValidator`, see [getting-started.md](getting-started.md#for-development-and-testing)) |
| 5 | Release build; on Windows, a GUI-subsystem exe with an embedded icon (skipped with `-Quick`) |

```powershell
.\tools\test_regressions.ps1 -Quick
```

`-Quick` takes about 20 s with a warm build cache. The full run also builds
the release exe. `-VerboseOutput` prints test output, and `-Glslang <path>`
points at a specific `glslangValidator`. It stops at the first failing step and
exits 1.

CI (`.github/workflows/ci.yml`) runs `cargo build --release --workspace` and
`cargo test --release --workspace` on Windows, Linux and macOS for every push
and PR. Run `cargo test --workspace` locally too, since it also covers the CPU
instruction tests in `crates/zakuro-cpu/tests` and the doc tests.

### Integration suites (`crates/zakuro/tests`)

| Suite | Guards |
| :--- | :--- |
| `settings_regression.rs` | Defaults, serialization, loading old and unknown `settings.toml` fields, per-game overrides |
| `resolution_presentation_regression.rs` | Resolution labels, filter mode ids (they must match the shader), screen placement for every layout |
| `gamepad_input_regression.rs` | Default bindings, stick flick thresholds, held-button suppression, menu input isolation, touch clamping |
| `hardware_architectural_regression.rs` | GPU vertex shading classification, Cranelift stack probing, cheat file parsing, texture pack naming, save format checks |
| `timing_fps_regression.rs` | CPU clock, cycles per frame, audio rate, presentation intervals, emulation decoupled from the display |
| `cli_regression.rs` | Command-line parsing |

When you fix a bug that a test could have caught, add the test here or next to
the code.

## Sandbox

Never run tests against the user's real data. Point Zakuro's data root at a
scratch folder for the whole test:

| OS | Variable | Zakuro's data | Recompiled code |
| :--- | :--- | :--- | :--- |
| Windows | `APPDATA` | `<scratch>\zakuro` | `<scratch>\3dsrecomp` |
| Linux | `XDG_DATA_HOME` | `<scratch>/zakuro` | `<scratch>/3dsrecomp` |
| macOS | `HOME` | `<scratch>/Library/Application Support/zakuro` | `…/3dsrecomp` |

```powershell
$env:APPDATA = "<scratch>\appdata"
Start-Process .\target\release\zakuro.exe -ArgumentList '"<path to game from the user>"'
```

- Copy in only what the test needs: a save (`zakuro\user\savedata\<title id>\`),
  a cheats file, `settings.toml`.
- **Speed and timing tests need recompiled code.** Without it the game is
  interpreted and much slower, which hides or invents timing problems. Copy
  (don't link) `<ID>.dll` (`.so`/`.dylib`), `<ID>.addons` and `<ID>.hints`
  from the user's recompiled folder into the sandbox's, after asking, or
  recompile in the sandbox ([recompiling.md](recompiling.md)).
- To test Home with games in the library, hard-link the game into
  `<scratch>\zakuro\library` instead of going through Add games, which opens a
  native file dialog.
- Stop only processes started from this repo's `target/`, for example
  `Get-Process zakuro | Where-Object Path -like "*\target\release\zakuro.exe"`.

## Replays

`ZAKURO_REPLAY=<file>` plays scripted input from the first frame, and works
without window focus. The first line is `clock <n>`, the console clock the
run starts at, which keeps runs repeatable. Each later line is

```
<frame> <buttons hex> <circle x> <circle y> <touch x> <touch y>
```

The state holds until the next line. Touch is `-1 -1` for none, and the
circle pad goes from -1 to 1. Button bits: A=1, B=2, Select=4, Start=8, Right=10,
Left=20, Up=40, Down=80, R=100, L=200, X=400, Y=800.

`ZAKURO_RECORD=<file>` writes a replay of a real session.

`tools/replays/ultra-sun-overworld-walk.txt` taps A every two seconds from
frame 900 to 4500 to get through the title screen and continue a save, then
walks the circle pad in squares from frame 5600 to 7760. It needs Pokémon Ultra
Sun and a save past the intro, both from the user.

## Driving the UI by hand

- **Keyboard** (default bindings): A = `X`, B = `Z`, X = `S`, Y = `A`, L = `Q`,
  R = `W`, Start = `Enter`, Select = `Backspace`, D-pad = arrows, circle pad =
  `I`/`J`/`K`/`L`. `Escape` opens and closes the pause menu. In menus, arrows
  move, `Enter` selects, `Escape` goes back, `Q`/`E` change section, and `X`
  resets a game option.
- **Mouse**: clicks and drags on the bottom screen are touches.
- **Focus** (Windows): keys only reach Zakuro while its window is in front.
  Check `GetForegroundWindow()` before sending keys with `SendInput` or
  `keybd_event`. If Windows won't give it focus, use a replay.
- **Screenshots**: `PrintWindow` captures the window even when it's covered.
  `tools/padtest.ps1` has a `Shot` function to copy.

### Virtual controller (Windows)

`tools/padtest.ps1` drives a window with a virtual Xbox 360 pad through
`tools/padctl` and [ViGEmBus](https://github.com/nefarius/ViGEmBus/releases),
saving screenshots along the way:

```powershell
cargo build --release --manifest-path tools\padctl\Cargo.toml
.\tools\padtest.ps1 -Name zakuro -PathLike "*\target\release\zakuro.exe" -Prefix "<scratch>\shots\z" `
  -Steps "|home", "press RIGHT 90|right", "press A 90|opened", "press RB 90", "press B 90|closed"
```

Each step is a padctl command, optionally followed by `| name` to save a
screenshot after it. Commands: `press <btn>[+<btn>] <ms>`, `hold <btn> <ms>`,
`stick <x> <y> <ms>`, `trig L|R <0..1> <ms>`, `wait <ms>`. Buttons: A B X Y LB RB
START BACK GUIDE LTHUMB RTHUMB UP DOWN LEFT RIGHT.

- gilrs reads Windows.Gaming.Input, which only gives a program controller input
  while its window is in front. padtest attaches to the foreground thread's
  input (`AttachThreadInput`) and raises the window before every step.
- WGI samples the pad at intervals, so padctl waits 60 ms after each release.
  Shorter gaps get lost.
- padtest picks the process's biggest window. A native dialog opens as a second
  window and takes the presses; close it with `WM_CLOSE`.

## Measuring timing

See [timing-and-audio.md](timing-and-audio.md#measuring). In short:

```powershell
.\tools\timing_run.ps1 -Rom "<game from the user>" -Sandbox "<scratch>\appdata" -Out "<scratch>\run.txt"
```

It runs the overworld-walk replay for 140 s with `ZAKURO_TIMING` set, then
prints `tools/timing_report.py`'s summary: input → vblank, shown-frame
intervals, off-beat share, dropped frames, and sound after picture. Compare
runs on the same machine, with the same settings and the same recompiled
library. Note the machine in [devices.md](devices.md).

## Measuring picture quality

`tools/imgbench` runs the presenter's fragment shader off-screen through
Vulkan, and scores the results:

```bash
cargo build --release --manifest-path tools/imgbench/Cargo.toml
```

```
imgbench present <in.png> <W> <H> <outprefix> <modes> [--spv f] [--reps n] [--srgb]
imgbench ref <in.png> <W> <H> <out>       # Lanczos3 in linear light
imgbench cmp <test> <reference>           # PSNR, SSIM, edge energy ratio
imgbench crop <in> <x> <y> <w> <h> <zoom> <out>
```

It uses the repo's `present.frag.spv` unless `--spv` names another build.
`present` also reports GPU time per pass. The method behind the current
filters, an 8x render reduced in linear light as the reference, is in
[rendering.md](rendering.md#how-the-kernels-were-chosen). Its push-constant
layout must match `present_common.glsl`. Update `imgbench` when the presenter's
push constants change.

## Testing the installer

See [releasing.md](releasing.md#testing-the-installer).
