# Contributing

The rules are in [AGENTS.md](../AGENTS.md). This page explains how to follow
them.

## Workflow

0. **Onboard the device**: know which machine you're on and what
   [devices.md](devices.md) says about it
   ([AGENTS.md](../AGENTS.md#at-the-start-of-every-session-onboard-the-device)).
1. **Understand before changing.** Find the code with
   [architecture.md](architecture.md), read the doc for the area, and check
   [history.md](history.md) for earlier fixes nearby. Many things that look
   odd are deliberate.
2. **Reproduce.** For a game bug, get the game files from the user (never
   fetch them), run in the [sandbox](testing.md#sandbox), and note the steps or
   write a [replay](testing.md#replays).
3. **Make the smallest change that fixes the cause**, not the symptom. Don't
   mix unrelated changes into one commit.
4. **Add or update a test.** Pure logic gets a unit test next to the code
   (`#[cfg(test)] mod tests`). Frontend behaviour that must not regress (settings
   compatibility, timing constants, input isolation, layouts) goes in the suites
   in `crates/zakuro/tests/`.
5. **Verify** with clippy, the tests and `tools/test_regressions.ps1 -Quick`,
   and in-game when the change is visible or audible.
6. **Measure** when the change is about speed, latency or picture quality, and
   compare before and after on the same machine ([testing.md](testing.md)).
7. **Update the docs** in the same commit, briefly: what the part is for,
   the decisions and why, and how to test it. Leave the detail to the code
   ([AGENTS.md rule 5](../AGENTS.md#5-every-code-change-is-reflected-in-the-docs-key-decisions-not-detail)).
   Add a [history.md](history.md) entry for root causes and rejected
   approaches.

## Code style

- **Format** with `cargo fmt`, but only on the files you change, and only when
  the result matches the code around it. Some files are hand-formatted with
  long lines. Don't reformat whole files you didn't otherwise change.
- **Match the surroundings**: naming, error handling, how a module is laid
  out. Most of the code is upstream's, so follow its conventions.
- **Comments** explain *why*, in plain lower-case prose (`// the output waits
  out fast forward rather than run dry`). Every module starts with a `//!`
  line saying what it is. Doc comments on items say what the item is for,
  not how it works.
- **No dead code.** No `#[allow(dead_code)]`, unused functions kept "for
  later", placeholder constants to silence unused imports, or commented-out
  code. Test-only helpers live in `#[cfg(test)]`. The one exception is
  `pro_controller.rs`, whose report parsing runs on macOS only.
- **No new warnings.** `cargo clippy --workspace --all-targets -- -D warnings`
  is clean, and must stay clean. Fix a lint rather than `allow` it. If an
  `allow` is right (`clippy::large_enum_variant` on the presenter enum, for
  example), give the reason in a comment next to it.
- **Dependencies**: add one only when it earns its place, with an exact or
  minor version in the crate's `Cargo.toml`, or in `[workspace.dependencies]`
  when two crates share it. Commit the `Cargo.lock` change.
- **`unsafe`** only at FFI and graphics API boundaries, with a comment on what
  makes it sound.
- **Errors** reaching the user are full sentences that say what to do
  ("Restart the game to apply"). Log messages are lower-case and say what
  happened.
- **Platform code** goes behind `cfg(windows)` / `cfg(target_os = …)`, and
  the other platforms must still build. CI builds Windows, Linux and macOS.

## Areas that need extra care

### Saved states

`crates/zakuro-core/src/state/` serializes the whole console with postcard.
Postcard isn't self-describing: fields are read in order, with no names. A
state is tagged with `CARGO_PKG_VERSION` and refused if the version differs.

- Adding, removing, reordering or retyping a field in anything serialized (CPU,
  CP15, memory, kernel objects, services, GPU state) changes the format.
- Make such a change only together with a version bump, and list "save states
  from earlier versions don't load" in the release notes.
- If a field isn't needed but removing it would break the format, keep it with
  a comment saying so. `Cp15::icache_invalidated` and `invalidate_ranges` are
  examples. Then drop it at the next bump.

### Settings

`crates/zakuro/src/settings.rs` reads `settings.toml`, and every older file
must keep loading.

- New fields take `#[serde(default)]` (or a `default = "…"` function).
- Renamed enum values keep the old name as `#[serde(alias = "…")]`. Unknown
  values fall back to the default where the enum allows it.
- Behaviour that changes the meaning of a stored value goes into
  `Settings::migrate()`.
- A per-game override is stored only when it differs from the system value.
- Add a case to `crates/zakuro/tests/settings_regression.rs`.

### The UI

Every list in the UI is built from `Entry` rows, and all input goes through
one `handle(item, Op)` ([frontend.md](frontend.md)). Add a setting as a row in
`page_entries()` / `setting()`. Never write separate keyboard, mouse or
controller handling for it, or they drift apart. Every row needs a one-line
help text. Check it in both themes.

### Shaders

The `.spv` files are committed and embedded. After editing a shader, rebuild
its `.spv` with the command in [rendering.md](rendering.md#shaders) and
commit both. `tools/test_regressions.ps1` checks that every shader still
compiles, but doesn't rebuild the committed `.spv` files.

### Timing and audio

Frame pacing and audio buffering trade latency against smoothness and
dropouts. Before changing either, record a baseline with `ZAKURO_TIMING` on a
recompiled game, and compare after ([timing-and-audio.md](timing-and-audio.md)).
Put the numbers in the commit message.

### Upstream

Most of the code comes from `fearkov/zakuro`. Keep changes in upstream code
small and well-contained, so upstream fixes still merge. `main` mirrors
upstream. Merge it into `stable`, never the other way.

## Commits and pull requests

- Commit as yourself. No `Co-authored-by` lines, and no mention of AI or
  assistants anywhere: messages, PR text, release notes.
- Subject line in the imperative, 72 characters or less: "Fix the pause menu
  losing focus after a picker". The body says what changed and why, plus
  measurements when they matter.
- One logical change per commit where you can. A release commit only bumps
  the version.
- Before opening a PR: rebase on `stable`, run the pipeline, and describe how
  you tested, including the machine for anything measured
  ([devices.md](devices.md)).
