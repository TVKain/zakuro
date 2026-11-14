# Packaging and releasing

Releases of this fork are published by the maintainer from `stable`.
Contributors don't need this page, except to test the installer.

## What a Windows release contains

`packaging/package-windows.ps1` writes to `target\dist` (or `-OutDir`):

| File | Contents |
| :--- | :--- |
| `zakuro-setup-v<version>.exe` | The installer (`crates/zakuro-setup`), with `zakuro.exe` embedded |
| `zakuro-v<version>-windows-x64.zip` | `zakuro.exe` alone, for running without installing. Recompiling then needs Zig or a C compiler of your own |

`-NoBuild` packs what's already built. `-ToolsOnly` only downloads Zig (size
and SHA-256 checked) next to `target\release\zakuro.exe`, for recompiling with
a dev build.

Linux (AppImage, tar.gz) and macOS (`Zakuro.app`) bundles are built by
`.github/workflows/release.yml` with `packaging/appimage.sh` and
`packaging/macos-app.sh`.

## The installer (`crates/zakuro-setup`)

- An eframe program in the app's style. `build.rs` embeds `zakuro.exe` (from
  `ZAKURO_EXE`, otherwise `target\release\zakuro.exe`), an `asInvoker`
  manifest (so Windows doesn't auto-elevate a program called "setup"), and the
  icon. Build `zakuro` in release mode first.
- **Install** is per user and needs no admin. By default it goes to
  `%LOCALAPPDATA%\Programs\Zakuro`. It copies `zakuro.exe` and the licenses,
  copies itself as `uninstall.exe`, and downloads Zig 0.17.0 with `curl.exe`
  into `tools\zig-0.17.0` (checked; a matching `%TEMP%\zakuro-zig-0.17.0.zip`
  is reused; a failure is only a warning). It also makes Start menu and
  optional desktop shortcuts, and registers under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Zakuro`.
- **Uninstall** (`uninstall.exe`, or `--uninstall`) copies itself to `%TEMP%`
  and relaunches with `--dir`, so it can delete its own folder. It removes the
  shortcuts, the Installed apps entry, the program files (only from a folder
  that has `uninstall.exe`) and `%LOCALAPPDATA%\zakuro`. Checkboxes add games,
  saves and settings. `--quiet` keeps all data. The install folder comes from
  the registry unless the program really is the `uninstall.exe` beside
  `zakuro.exe`, so running setup with `--uninstall` from `target\` never
  deletes the build folder.
- **Controller** support (`pad.rs`): D-pad and left stick move egui's focus,
  A presses, B closes or cancels. Input only counts while the window is in
  front. `Setup::draw` takes the presses as input, so tests drive it without a
  controller.

### Testing the installer

Point `APPDATA`, `LOCALAPPDATA`, `TEMP` and `TMP` at a scratch folder before
running the setup exe. The Installed apps entry and desktop shortcut it makes
are real, so uninstall afterwards. `tools/padtest.ps1 -Name zakuro-setup`
drives it with a virtual controller ([testing.md](testing.md#virtual-controller-windows)).

## Publishing a release

1. Make sure `stable` is committed and pushed, and the full pipeline passes:
   `.\tools\test_regressions.ps1`.
2. Pick the version: a patch bump unless told otherwise.
3. Write `docs/release-notes/v<version>.md` from `git log v<last>..stable`,
   and commit it. Use the sections *What's new*, *Changes*, *Fixes* and
   *Download*. Wording is for players, not developers. List only what changed
   since the last release. Copy the Download section from the previous notes.
   If saved states break, say so. **No mention of AI or assistants**: the
   script refuses the notes if there is any.
4. Run `.\packaging\release.ps1 -Version <version>`. It takes about 10 minutes,
   so run it in the background. It:
   - checks that you're on `stable` with a clean tree, that the tag and GitHub
     release don't exist yet, and that nothing in `target\release` is running;
   - bumps `[workspace.package] version` and `Cargo.lock`;
   - runs `tools\test_regressions.ps1 -Quick`;
   - packages into `target\dist`;
   - commits "Release v<version>" with your git identity, pushes `stable`, and
     publishes the GitHub release (`gh`, repo `-Repo`, default `TVKain/zakuro`)
     with both files.

   Any failure before the commit undoes the version bump.
5. Report the release URL it prints.

`-DryRun` does everything except commit, push and publish, into
`target\release-dry`.
