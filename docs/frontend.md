# Frontend

`crates/zakuro`: the window, the interface, input, settings and the library.
The UI is drawn with egui into an overlay that the presenter draws over the
screens.

## Screens

- **Home.** A row of large game tiles (the selected one outlined with a
  breathing accent, name above, details below), plus round dock buttons: Add
  games, Multiplayer, System Settings, Quit. While a recompile runs, a card at
  the top shows the game, stage, percent, time left, and how many games are
  waiting.
- **System Settings** and a game's **Options** share one window: a sidebar
  of sections, a list of rows, a round close button, and a help strip at the
  bottom for the selected or hovered row.
- **Pause menu** over a running game (Escape, or the controller's
  Menu/Home button, configurable under Controls).
- **Pickers** (`SelectPopup`) list the choices with a note for each and a
  check on the one in force. **Prompts** (`Ask`, remove, delete) look the same
  but have no check.
- **Cheats**, **Add to Zakuro** (import), messages and toasts.

The window always opens maximized. Fullscreen is a System Settings toggle
(and F11), and isn't saved. Leaving fullscreen restores the maximized
state.

## One row model, one input path

Every list (settings, options, pause menu, pickers, cheats, import) is a
`Vec<Entry>`, rebuilt from `Settings` each frame (`page_entries()`,
`setting()` in `menus.rs`). A row (`Row`) has a label, a `Value` (switch,
text, range with arrows, more, check), an optional note saying why it can't
apply, and a one-line `help`. Rendering, hover, click and button navigation
all act on the same items through `handle(item, Op)`, where `Op` is
`Activate`, `Step(±1)` or `Reset`. Keyboard, mouse, touch and controller
therefore can't drift apart. **Add new settings as rows. Never give them
separate input handling.**

`nav.rs` maps keys and buttons to one `Nav` vocabulary, queued and applied at
frame time. `InputMode` switches automatically:

- **Pointer** (mouse movement, clicks, touch, wheel): hover selects, button
  hints are hidden, clickable widgets show a pointing-hand cursor.
- **Directional** (keys, buttons): a highlight is shown, the cursor is hidden,
  and the hints show keyboard or controller glyphs. Held directions repeat.

### Isolation

While any menu is over the game (`Menus::over_game()`):

- The game gets neutral input every frame. Hotkeys and microphone blowing are
  off. Pointer events never reach the touchscreen (`pointer_taken`).
- Escape works down the stack: picker → window → pause menu.
- On resume, buttons and sticks still held are ignored until released
  (`suppress_current_held()` in `gamepad.rs`), so the press that closed a
  menu never reaches the game.

## Settings (`settings.rs`)

`Settings` holds the system values, and `GameSettings` holds per-game
overrides (`Option<T>`). A game's Options show the value in effect. Changing
it stores an override only when it differs from the system value (shown with
a dot), and X or right-click resets it. `Settings::migrate()` drops overrides
equal to the system value and settings that no longer exist. See
[contributing.md](contributing.md#settings) for compatibility rules.

The Graphics page has Resolution, Display filter, Upscale textures and Custom
textures. Under **Motion**: Frame rate limit and Frame interpolation. Under
**Advanced**: Graphics API and 3D rendering (GPU/CPU). Most settings apply at
once:

- The graphics API switches on Home, by recreating the window
  (`App::switch_presenter`). While a game runs, its row is disabled with
  "Change it from Home".
- Custom textures toggle in a running game.
- Language, recompiled code and 3D rendering need the game restarted. Their
  rows say "Restart the game to apply" only when the value differs from what
  the game started with (`App::started`).

## Themes

`Settings::theme` is System (default), Dark or Light, under System Settings →
Display. `Gui::set_theme` sets egui's preference and the Windows title bar
(`None` for System, so Windows keeps reporting changes). The palette is in
`menus.rs` `pub mod color`: slate neutrals with one violet accent, and text at
4.5:1 contrast or better in both themes. Colours are functions
(`color::text()`, `color::accent()`…) that read the current theme, so never
hard-code a colour. Text over game art or black overlays stays white. Check
every UI change in both themes.

## Icons (`icons.rs`)

24×24 outline icons as SVG path data, parsed with kurbo and stroked with
tiny-skia (width 2, round caps). They're drawn at the screen's real pixel
size, pixel-aligned, and cached as egui textures per size. Recreating the
presenter calls `Menus::forget_textures()`.

## Library and store

- Games live only in Zakuro's library, `<data>/library` (`store::library_dir`).
  There's no setting to move it. An old `games = '…'` line in `settings.toml`
  is read once: its games open in Add to Zakuro to be copied in.
- **Add games** (or drag and drop onto the window) identifies games, updates,
  DLC, zipped games, texture packs (folder or zip) and Citra cheat files, and
  installs them in the background with progress (`store.rs`). Texture packs are
  copied once into `<data>/textures` and junction-linked into each chosen
  game's `mods/<id>/textures`.
- **Remove this game** (Options → Add-ons → Files) deletes the game and its
  updates, DLC and Download Play children from the library. Saves, mods and
  recompiled code stay.
- Starting a game that needs recompiling asks: recompile, Play without, or
  Don't ask again for this game ([recompiling.md](recompiling.md)).

## Input devices

- **Keyboard**: `input.rs`. Defaults are listed in [testing.md](testing.md#driving-the-ui-by-hand).
- **Controllers**: `gamepad.rs` through gilrs, and `pro_controller.rs` for the
  Switch Pro Controller on macOS. Stick flicks for menu navigation use
  hysteresis (`update_axis_flick`: ±0.55 to activate, ±0.25 to reset).
- **Touch**: `WindowEvent::Touch` drives the bottom screen like the mouse.
- **Replays**: `ZAKURO_REPLAY` replaces live input ([testing.md](testing.md#replays)).

## Windows specifics

- `main.rs` sets `windows_subsystem = "windows"`, so no console window
  opens. `-d` / `--debug` (and `--help`) attach to the parent console or
  allocate one.
- `build.rs` embeds the icon (`packaging/zakuro.rc`) and asks for an 8 MiB
  main-thread stack, because guest code runs on it.
- The log goes to `<data>/zakuro.log` (`report.rs`).
