# Zakuro developer docs

The rules for changing the code are in [`AGENTS.md`](../AGENTS.md) at the
repository root. Read them first.

| Doc | What's in it |
| :--- | :--- |
| [getting-started.md](getting-started.md) | Dependencies per OS, building, running, the files you need for testing |
| [architecture.md](architecture.md) | The crates, the important modules, and how a frame flows from game code to the screen |
| [contributing.md](contributing.md) | How to make a change: code style, the areas that need extra care, tests, commits |
| [testing.md](testing.md) | The regression pipeline, the sandbox, replays, driving the UI, measuring timing and picture quality |
| [rendering.md](rendering.md) | Hardware rasterizer, presenters, display filters, texture upscaling, shaders |
| [timing-and-audio.md](timing-and-audio.md) | Frame pacing, input lag, the audio path, lip sync |
| [frontend.md](frontend.md) | Menus, unified input, settings, themes, library and store |
| [recompiling.md](recompiling.md) | 3dsrecomp, the vendored copy and its patches, Zig, when a game counts as outdated |
| [releasing.md](releasing.md) | Packaging, the installer, publishing a release |
| [devices.md](devices.md) | The machines changes were tested on, and what each showed |
| [history.md](history.md) | Past bugs, root causes, measurements and decisions |
| [release-notes/](release-notes/) | Notes published with each release |

The user-facing documentation is the [README](../README.md). Tested games are
listed in [COMPATIBILITY.md](../COMPATIBILITY.md).
