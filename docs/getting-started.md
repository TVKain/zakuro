# Getting started

## Dependencies

Agents may install anything on this page without asking, from official
sources or the OS's package manager, and should say what they installed
([AGENTS.md](../AGENTS.md#8-installing-dependencies)). Typical commands:

```powershell
winget install --id Rustlang.Rustup -e
```

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

```powershell
winget install --id KhronosGroup.VulkanSDK -e
```

```powershell
winget install --id Python.Python.3.12 -e
```

```powershell
winget install --id GitHub.cli -e
```

### All platforms

- **Rust**, through [rustup](https://rustup.rs). `rust-toolchain.toml` picks
  the stable channel with `rustfmt` and `clippy`. The workspace needs Rust 1.95
  or newer (`rust-version` in `Cargo.toml`).
- **git**.
- **A Vulkan driver** to use the GPU renderer and the Vulkan presenter.
  Without one, Zakuro presents with OpenGL and draws 3D on the CPU.

### Windows

- **Visual Studio Build Tools** (or Visual Studio) with the *Desktop
  development with C++* workload, for the MSVC linker and the Windows SDK. The
  SDK's `rc.exe` compiles the program's icon. If it's missing, `build.rs`
  tries `windres`, then falls back to the committed `packaging/zakuro.res`.
- **PowerShell 5.1** (built in) runs the scripts in `tools/` and `packaging/`.

### Linux

pkg-config plus the ALSA and udev development files:

```bash
sudo apt-get install -y pkg-config libasound2-dev libudev-dev   # Debian, Ubuntu
sudo pacman -S alsa-lib systemd-libs pkgconf                    # Arch
```

The font tests draw Japanese text and want a CJK font
(`fonts-noto-cjk` on Debian and Ubuntu).

### macOS

The Xcode command line tools (`xcode-select --install`). For Vulkan, install
MoltenVK and the Vulkan loader (`brew install molten-vk vulkan-loader`).
Without them, Zakuro presents with OpenGL and draws on the CPU.

### For development and testing

Only needed for the tasks listed:

| Tool | Needed for | Get it |
| :--- | :--- | :--- |
| `glslangValidator` | Recompiling shaders; step 4 of the test pipeline | The [Vulkan SDK](https://vulkan.lunarg.com/sdk/home) (sets `VULKAN_SDK`), a [glslang release](https://github.com/KhronosGroup/glslang/releases) unpacked into `tools/glslang/` (git-ignored), or your package manager (`glslang-tools`, `glslang`) |
| A C compiler | Recompiling games with 3dsrecomp | Windows: Zig 0.17.0 in `target\release\tools\zig-0.17.0` via `packaging\package-windows.ps1 -ToolsOnly`. Elsewhere: gcc or clang on `PATH` |
| [ViGEmBus](https://github.com/nefarius/ViGEmBus/releases) | Driving the UI with a virtual Xbox controller (`tools/padtest.ps1`, Windows only) | Install the driver, then build `tools/padctl` |
| Python 3 | `tools/timing_report.py`, which analyses frame-timing records | python.org or your package manager |
| [GitHub CLI](https://cli.github.com) (`gh`) | Publishing releases (maintainer only) | cli.github.com |

The tools in `tools/` are separate Cargo projects, outside the workspace.
Build them with `--manifest-path`, for example
`cargo build --release --manifest-path tools/padctl/Cargo.toml`.

## Build and run

```bash
cargo build --release
```

```bash
cargo run --release -- path/to/game.3ds
```

Without a game, Zakuro opens Home, its library. `zakuro --help` lists the
options. On Windows the release program is a GUI app with no console, so use
`-d` / `--debug` to attach one and see the log. The log is also written to
`zakuro.log` in the data folder.

Debug builds work, but games run far below full speed in them. Use `--release`
for anything you look at or time.

The examples in `zakuro-core` (`boot`, `trace`, `watch`, `probe`,
`dump_archive`) and `zakuro-fs` (`romdump`, `dumpcode`) run the core without
the frontend. Most give their usage in their first line:

```bash
cargo run --release -p zakuro-core --example boot -- path/to/game.3ds 600
```

```bash
cargo run --release -p zakuro-fs --example romdump -- path/to/game.3ds
```

## Files you need to test in a game

The repository has no game data and never will. To test anything in a real
game, you need your own decrypted dumps: a game, and optionally its update,
DLC, a save, a texture pack or cheats. Agents must ask the user for paths to
these instead of searching for them or downloading them (see
[AGENTS.md](../AGENTS.md#1-ask-for-game-files-never-fetch-them)).

Games run interpreted until they're recompiled. That's fine for correctness,
but for anything about speed or timing, recompile the game first (see
[recompiling.md](recompiling.md)), or copy an existing recompiled library into
the sandbox ([testing.md](testing.md#sandbox)).

## First checks

```powershell
.\tools\test_regressions.ps1 -Quick
```

On Linux or macOS, run the same steps by hand:

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```
