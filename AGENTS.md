# AGENTS.md

Guide for anyone, human or agent, changing Zakuro. Read it before you edit code,
build, test or commit. The details live in [`docs/`](docs/README.md); this file
holds the rules and tells you where to look.

Zakuro is a high-level-emulated (HLE) Nintendo 3DS emulator in Rust. Game code
runs either through an ARM11 interpreter or from a library that
[3dsrecomp](https://github.com/fearkov/3dsrecomp) recompiled ahead of time. The
GPU (PICA200) is drawn through Vulkan, or on the CPU, and shown through Vulkan
or OpenGL. This repository is the `TVKain/zakuro` fork. Its `stable` branch
carries the handheld, presentation, latency and UI work on top of
`fearkov/zakuro`.

## Start here

| You want to | Read |
| :--- | :--- |
| Install the tools, build and run | [docs/getting-started.md](docs/getting-started.md) |
| Find where something lives | [docs/architecture.md](docs/architecture.md) |
| Make a change the right way | [docs/contributing.md](docs/contributing.md) |
| Test a change, including in-game | [docs/testing.md](docs/testing.md) |
| Work on the picture, shaders or filters | [docs/rendering.md](docs/rendering.md) |
| Work on frame pacing, input lag or sound | [docs/timing-and-audio.md](docs/timing-and-audio.md) |
| Work on menus, settings, input or the library | [docs/frontend.md](docs/frontend.md) |
| Work on recompiling or the vendored 3dsrecomp | [docs/recompiling.md](docs/recompiling.md) |
| Package, install or release | [docs/releasing.md](docs/releasing.md) |
| See which machines things were tested on | [docs/devices.md](docs/devices.md) |
| Learn why something is the way it is | [docs/history.md](docs/history.md) |

## At the start of every session: onboard the device

Zakuro depends heavily on the platform. Drivers, GPU vendor, display refresh
rate, CPU core layout and OS all change crashes, speed, frame pacing and
audio. Before changing or measuring anything, find out which machine you're
on:

1. **Identify it.** Gather the OS and build, CPU, GPU and driver version,
   displays with resolution and refresh rate (and VRR, if you can tell), RAM,
   and the machine model. The commands are in
   [docs/devices.md](docs/devices.md#identifying-the-machine).
2. **Match it** against [docs/devices.md](docs/devices.md).
   - **Known device:** read its notes. Use its earlier results as the baseline
     for any comparison.
   - **New device:** add a section for it. If the model can't be read
     automatically, ask the user. Never compare its measurements with another
     device's.
3. **Check the toolchain** for the task (Rust, the platform build tools,
   `glslangValidator`, and so on), and install what's missing (rule 8).
4. **Tell the user** in one line which device you're on and whether it's new.

Record every measurement you report together with the device it came from.

## Rules

### 1. Ask for game files, never fetch them

Nothing copyrighted is in this repository, and nothing copyrighted may be added.
Testing a change in a real game needs files only the user can provide. **When a
task needs one of these, ask the user for its path. Don't search the disk for
it, guess, or download it.**

| Asset | Needed for |
| :--- | :--- |
| A decrypted game (`.3ds`, `.cci`, `.cxi`, or `.cia`) | Anything that boots a game: rendering, audio, timing, input, saves |
| A decrypted update or DLC (`.cia`) | Update or DLC handling, builds in `enhancements.txt` |
| A save that gets past the game's intro | Replays such as `tools/replays/ultra-sun-overworld-walk.txt` |
| A recompiled library (`<ID>.dll`/`.so`/`.dylib`, plus `.addons` and `.hints`) | Realistic speed and timing. Without one, the game is interpreted and much slower. |
| A texture pack, or a Citra/Azahar cheat file | Custom textures, cheats |

Ask once per session and reuse the answer. Never commit these files. Never
copy them into the repository, not even into `target/`. If the user has none,
say what you couldn't verify.

### 2. Never touch the user's real data

Zakuro keeps settings, saves, the library and logs in its data folder, and
recompiled code next to it:

| OS | Zakuro's data | Recompiled code |
| :--- | :--- | :--- |
| Windows | `%APPDATA%\zakuro` | `%APPDATA%\3dsrecomp` |
| Linux | `$XDG_DATA_HOME/zakuro` (`~/.local/share/zakuro`) | `…/3dsrecomp` |
| macOS | `~/Library/Application Support/zakuro` | `…/3dsrecomp` |

Run every test with that root pointed at a scratch folder: `APPDATA` on
Windows, `XDG_DATA_HOME` on Linux, `HOME` on macOS. See
[docs/testing.md](docs/testing.md#sandbox). Copy in only what the test needs.
Never write to, move or delete the real folders, and above all never touch
their saves (`user/savedata/…`, `states/`).

Only stop Zakuro processes whose executable is under this repository's
`target/` folder. The user may be playing with an installed copy.

### 3. Leave the tree green

Before saying a change is done:

1. `cargo clippy --workspace --all-targets -- -D warnings` must be clean.
2. `cargo test --workspace` must pass, or at least the crates you touched plus
   `-p zakuro --test "*"`.
3. On Windows, run `tools\test_regressions.ps1 -Quick`. It runs clippy, the
   unit and integration tests, and shader compilation. Run it without `-Quick`
   before a release.
4. If the change is visible or audible in a game, test it in-game in the
   sandbox, as described in [docs/testing.md](docs/testing.md), with the
   files you asked for. If you couldn't, say so.

Don't weaken a test to make it pass. If a test is wrong, fix it and say why.

### 4. Changes that need extra care

- **Saved states.** `zakuro-core::state` writes the console with postcard,
  which has no field names, so adding, removing or reordering a field of
  anything serialized (`Cpu`, `Cp15`, GPU state, services) breaks older
  states. States are tagged with the crate version, so mismatches are refused
  rather than misread. Change such a struct only with a version bump, and say
  so in the release notes.
- **Settings.** `settings.toml` must keep loading. New fields need
  `#[serde(default)]`, renamed values need an alias or an entry in
  `Settings::migrate()`, and `crates/zakuro/tests/settings_regression.rs`
  needs a case for the change.
- **Shaders.** The program embeds the committed `.spv` files. If you edit a
  `.glsl`, `.frag`, `.vert` or `.comp` file, recompile its `.spv` and commit
  both. [docs/rendering.md](docs/rendering.md#shaders) has the command for
  each variant. `present_common.glsl` is shared with the OpenGL presenter, so
  it must stay valid GLSL 450 and GLSL 330 core.
- **Vendored 3dsrecomp.** `vendor/3dsrecomp` carries local patches. Re-apply
  them when you update it ([docs/recompiling.md](docs/recompiling.md)).
- **Performance.** Before you change frame pacing, audio buffering, the
  presenter or the rasterizer, measure with `ZAKURO_TIMING` or `imgbench`.
  Put the numbers in the commit message and in the doc you update.

### 5. Every code change is reflected in the docs: key decisions, not detail

A commit that changes behaviour, defaults, file locations, settings, tools,
the build or the architecture updates the matching file in `docs/` **in the
same commit**. Keep it short. The code and its comments are the detailed
reference, and the docs only say what a newcomer couldn't easily work out from
them:

- **Write down:** what a part is for and where it lives; decisions and the
  reason for each ("1280-sample cushion: smaller sizes dropped out on the
  Claw"); constraints that aren't obvious (formats that must stay
  compatible, rules between modules); measurements behind a decision, with the
  device; and how to test or measure the area.
- **Leave out:** walkthroughs of the code, every function or field, numbers
  that will drift (line counts, exact sizes), and anything that only repeats
  a comment.
- **Fix or remove** any statement your change made false. Out-of-date docs
  are worse than missing ones.

Add one short entry to [docs/history.md](docs/history.md) when a future
contributor would otherwise have to rediscover something: a root cause, a
rejected approach, or a decision and why. Results from a device go in
[docs/devices.md](docs/devices.md) under that device.

A change with no effect outside the code, such as a refactor or a test,
needs no doc update.

### 6. Commits

- Commit as yourself, using your own `user.name` and `user.email`.
- **Never add `Co-authored-by` lines.**
- **Never mention AI, LLMs, prompts or assistants** in commit messages, PR
  descriptions or release notes. `packaging/release.ps1` refuses release
  notes that do.
- Write messages in the imperative, concise and specific, for example
  "Lower audio latency with a pass-through stretcher". Explain why in the
  body when it isn't obvious.
- Development happens on `stable`. `main` mirrors upstream `fearkov/zakuro`.
- Don't push, open PRs, tag or release unless asked. Releases are made by the
  maintainer with `packaging/release.ps1` ([docs/releasing.md](docs/releasing.md)).

### 7. Code style

Covered in [docs/contributing.md](docs/contributing.md#code-style). In short:
match the code around you; comments explain why, in plain lower-case prose;
leave no dead code, no `allow(dead_code)`, and no placeholder constants; add
no new dependency without a reason.

### 8. Installing dependencies

You may install the development dependencies a task needs without asking.
That covers everything in
[docs/getting-started.md](docs/getting-started.md#dependencies): the Rust
toolchain and components, platform build tools, the Vulkan SDK or glslang,
Python, ViGEmBus, the GitHub CLI, and Linux packages through the system
package manager.

- Install only from the official source or the OS's package manager
  (`winget`, `apt`, `pacman`, `brew`, `rustup`), never from mirrors or
  unofficial builds.
- Prefer per-user installs. When an installer needs administrator rights, the
  user approves the system prompt.
- Tell the user what you installed and why, in one line each.
- Dependencies of the program itself are a code change: a new crate goes
  through review like any other change
  ([docs/contributing.md](docs/contributing.md#code-style)).
- Never install anything that provides game data, keys or firmware (rule 1).
