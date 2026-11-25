# ztrx

**ztrx** runs the Xbox Series build of the game natively on Windows PCs with
NVIDIA graphics cards, from game files you supply yourself.

**This repository contains no game files or assets.** You need your own copy
of the game. Not affiliated with or endorsed by its developer or publisher.

## Status (2026-11-25)

Tested on Windows 11 with an **NVIDIA GeForce RTX 3050** (8 GB):

| | |
|---|---|
| Boot, first-boot screens, intro, opening credits | works |
| Prologue | playable from the cell to the boat chase: cell, cafeteria, robbery, police chase (wanted level, minimap), boat, binoculars |
| Stability | 1 h 30 min+ in one session without a crash |
| Frame rate | 1-3 fps in the world (the first run is slower while shaders translate; they are cached after that) |
| Known problems | anything in shadow is black and bright scenes overexpose (the game's ray-traced lighting does not run yet); some blocky artifacts; boats sit low in the water |

## Requirements

- Windows 10/11 x64, a CPU with AVX2
- A Direct3D 12 GPU with Shader Model 6.6 and mesh-shader support for the current renderer. GPUs without mesh shaders (including the GTX 1050 Ti) cannot run mesh pipelines until the compute/vertex fallback is completed.
- Rust (the version in `rust-toolchain.toml`)
- Several GB of free Windows commit memory (raise the paging file if `crashes.log` reports error 1455)

## Build

```powershell
cargo build --release -p victory
```

## Play

```powershell
powershell -ExecutionPolicy Bypass -File scripts\play.ps1 -Game "<your game folder>\<the game's exe>"
```

`scripts\play.ps1` keeps the shader cache and saves in folders you choose
(`-Cache`, `-Data`) and presses no buttons for you. Controller: XInput pads
(a DualShock/DualSense through DS4Windows works). With no pad connected the
keyboard acts as one while the game window has focus.

## Faster settings (optional, off by default)

`VICTORY_RELAY=2`, or `VICTORY_LATE_WAIT=3 VICTORY_WIDE_BACK=1` cut frame
time on the test machines. Set them in PowerShell before running the script.

## Debugging switches

| Switch | Effect |
|---|---|
| `VICTORY_CAPTURE=<dir>` + `VICTORY_CAPTURE_FRAMES=<list>` | save those frames as PNG (diagnostics build) |
| `VICTORY_KEYS=30:Enter,35:X` | press keys at those seconds (diagnostics build) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, report the commands in flight |
| `VICTORY_PIPELINE_WAIT_MS=24` | shorter per-frame wait for pipelines still being built |

Diagnostics build: `cargo build --profile diag -p victory --features diagnostics`

## Mesh-shader fallback progress (2026-11-25)

The current renderer still requires native D3D12 mesh shaders. The GTX 1050 Ti
does not provide that feature, so it is not supported by the current mesh
pipeline path. Disabling the capability check would only hide the failure; it
would not turn mesh dispatches into ordinary draws.

The first fallback-port milestone is a read-only workload inventory:
- Run `cargo run --release -p victory -- mesh-census "<game folder>"` to
  count mesh programs by group size, wave size, LDS use and declared output
  limits. Add `--list` for per-program numeric metadata.
- See [docs/mesh-fallback-port.md](docs/mesh-fallback-port.md) for the
  implementation sequence, upstream references and acceptance criteria.

Research has identified upstream compute/vertex fallback work in
[PR #961](https://github.com/Marvisak/VIctory/pull/961) and a newer
[draft port, PR #2203](https://github.com/Marvisak/VIctory/pull/2203).
Those are useful technical references, not proof of a playable fallback.
This repository is a separate source snapshot; the upstream patches cannot be
safely copied wholesale. PR #2 adds the experimental compute-emission API and
generated vertex shader. Draft PR #3 adds the first direct-dispatch GPU path:
compute/graphics PSOs, output bindings, per-list scratch buffers, barriers and
triangle draws. **PR #3 is still under CI review and has not been tested on a
physical GPU.** Indirect mesh dispatches and wave64 behavior remain unfinished,
so the GTX 1050 Ti fallback is not yet considered usable or playable. See
[docs/mesh-fallback-port.md](docs/mesh-fallback-port.md) for status and
acceptance gates.

## License

GPL-3.0-or-later (see `LICENSE`). Based on VIctory (Marvisak) and victory-fork (fxceless).
