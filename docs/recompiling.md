# Recompiling

Zakuro runs game code either in the ARM11 interpreter (`zakuro-cpu`) or from a
native library that [3dsrecomp](https://github.com/fearkov/3dsrecomp) built
from the game ahead of time. Recompiled games run at full speed. Interpreted
ones run well below it. Anything the library doesn't cover still goes through
the interpreter.

## Pieces

| Where | What |
| :--- | :--- |
| `vendor/3dsrecomp` | The recompiler (crate `recomp3ds`) and `abi/` (`recomp_abi`, the interface the library and Zakuro share). Outside the workspace, used as a path dependency |
| `crates/zakuro/src/recompile.rs` | Runs a recompile on a background thread, works out what a game's update, DLC and mods change, reports progress |
| `crates/zakuro/src/zig.rs` | Finds the bundled C compiler |
| `crates/zakuro-core/src/recompiled.rs` | Loads the library and calls into it from the CPU loop |
| `crates/zakuro-core/src/hints.rs` | Records where a game ran in the interpreter despite having a library, for the next recompile |

Libraries live in `<data>/../3dsrecomp/<ID16>.dll` (`.so`, `.dylib`), next to
`<ID16>.hints` and `<ID16>.addons`. On Windows that's `%APPDATA%\3dsrecomp`.
Intermediate objects go to `3dsrecomp/<ID16>` in the cache folder
(`%LOCALAPPDATA%`, `~/Library/Caches`, or `$XDG_CACHE_HOME`).

## C compiler

The first one found is used:

1. Zig 0.17.0 in `tools/zig-0.17.0` next to `zakuro.exe`. The installer
   downloads it there. For a dev build, run
   `packaging\package-windows.ps1 -ToolsOnly`, which puts it next to
   `target\release\zakuro.exe`.
2. `%LOCALAPPDATA%\zakuro\tools\zig-0.17.0` on Windows (`<data>/tools`
   elsewhere), where older versions downloaded it.
3. gcc or clang on `PATH`.

## When a game counts as outdated

- The library's ABI generation is older than `recomp_abi::GENERATION`.
- The game's update or DLC changed. `library.rs` writes `<ID16>.addons`,
  listing the update and DLC (id and version) the library was built with.
  Games recompiled before this stamp existed get one written as they are now.
- A mod that changes code (`code.bin`, `code.ips`/`.bps`, `exheader.bin`) needs
  a recompile with the mod in place. Other mods change nothing that gets
  recompiled.

## Rules the frontend enforces

- One recompile at a time. The others queue (`App::waiting`).
- While any recompile runs or waits, no game starts. The Home tile,
  `Action::Play` (which every prompt answer goes through) and joining Download
  Play all check `recompile::blocking_play`.
- A recompile starts only from Home. Over a running game, the Performance rows
  and "Recompile N outdated games" are hidden. Going Home ends the game, so a
  game and a recompile never run together.
- Starting a game that isn't recompiled (or is outdated) asks: recompile, Play
  without, or Don't ask again for this game (`Settings::dont_ask_recompile`).
  After an install, the games it added that need recompiling are offered
  together.

## Local patches to the vendored 3dsrecomp

`vendor/3dsrecomp` is upstream `fearkov/3dsrecomp` at `df82e6e`, plus these
changes. **Re-apply them whenever you update the vendored copy:**

1. **Hidden compiler windows** (`src/compile.rs`). Zakuro is a GUI-subsystem
   program, so every compiler, linker and `ar` process would otherwise open its
   own console window, hundreds per recompile. `hide()` sets
   `CREATE_NO_WINDOW` on each command, and `lower()` also sets
   `BELOW_NORMAL_PRIORITY_CLASS`.
2. **No debug info** (`src/compile.rs` `FLAGS`, `-g0`). zig cc writes CodeView
   debug info by default. Without it, `.text` is byte-identical. A full Ultra
   Sun build with 7 jobs went from 1055 s to 726 s, and objects from 1442 MB to
   324 MB.

Test a recompile in the [sandbox](testing.md#sandbox): the library goes to the
sandbox's `3dsrecomp` folder.
