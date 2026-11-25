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
- A Direct3D 12 GPU. Native mesh-shader hardware is required for the stable renderer; a compute/vertex fallback is under experimental development for GPUs such as the GTX 1050 Ti and is not yet runtime-validated.
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

The GTX 1050 Ti does not support D3D12 mesh shaders, so it cannot execute
the current native mesh pipeline. Disabling the capability check would only
hide the failure; mesh dispatches must be translated into compute and draws.

Completed milestones:
- **PR #1:** added the `victory mesh-census` diagnostic. It reports native
  mesh thread groups, wave size, LDS use and output limits.
- **PR #2:** added an opt-in shader-emission API that converts a mesh shader
  into a compute producer and generated vertex shader. The existing native
  translation path remains unchanged by default.
- **Direct dispatch branch:** now builds a compute PSO and a generated
  vertex/pixel PSO, reserves bounded per-list UAV scratch regions, dispatches
  the compute shader, inserts UAV/resource barriers and issues an indexed or
  expanded triangle draw. It is automatically selected on hosts without mesh
  shader support (or can be forced with `VICTORY_MESH_FALLBACK=1` on a
  supported GPU for testing).

**Important: the direct-dispatch path is still experimental and has not been
validated on a real non-mesh GPU.** Indirect mesh dispatch is explicitly
rejected for now; wave64 emulation, per-primitive output equivalence, VRAM
pressure and actual title rendering still need validation. The direct path
also caps a single scratch allocation at 256 MiB to avoid unbounded requests.
See [docs/mesh-fallback-port.md](docs/mesh-fallback-port.md) for the exact
status, implementation sequence and acceptance criteria.

Upstream references:
- [Marvisak/VIctory PR #961](https://github.com/Marvisak/VIctory/pull/961)
- [Marvisak/VIctory draft PR #2203](https://github.com/Marvisak/VIctory/pull/2203)

Those PRs are technical references, not proof that this fork or the target
game is playable on the GTX 1050 Ti. Windows CI only checks compilation,
Clippy and tests; it does not validate D3D12 output or GPU performance.
## License

GPL-3.0-or-later (see `LICENSE`). Based on VIctory (Marvisak) and victory-fork (fxceless).
