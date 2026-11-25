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
- A Direct3D 12 GPU with Shader Model 6.6 and Resource Binding Tier 3 for the experimental compute/vertex mesh fallback, or native D3D12 mesh-shader support for the native path. Runtime/driver feature support must be verified on the target PC.
- Rust (the version in `rust-toolchain.toml`)
- Several GB of free Windows commit memory (raise the paging file if `crashes.log` reports error 1455)

## Build

```powershell
cargo build --release -p victory
```

The executable is configured for D3D12 Agility SDK **1.620.2**. The first
launch through `scripts/play.ps1` checks that the matching app-local runtime
is beside `victory.exe` and runs `cargo xtask agility` if it is missing.
That command downloads the pinned Microsoft NuGet package and verifies its
SHA-512 before staging the runtime. Internet access is needed for the first
staging run. When launching `victory.exe` directly, run
`cargo xtask agility` from the repository root first.

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

An experimental compute/vertex fallback is now wired into the GPU pipeline for
adapters without native D3D12 mesh shaders. Mesh programs are lowered to
compute shaders that write bounded output buffers; generated vertex shaders
then render those outputs using conventional draw commands. Direct and
indirect mesh dispatch conversion are implemented, including the console
`DispatchMeshX` indirect entry point.

The fallback is **not yet validated as a playable configuration**. CI verifies
the code and synthetic tests, but no physical GTX 1050 Ti run or output-image
equivalence test has passed on this repository snapshot. Some wave64 programs
remain unsupported, some shader translations are still refused, and the
GTX 1050 Ti's 4 GB VRAM is a substantial constraint. Shader Model 6.6 and
Resource Binding Tier 3 support also depend on the installed D3D12 runtime and
driver; Windows 10 may require a suitable Agility SDK runtime.

See [docs/mesh-fallback-port.md](docs/mesh-fallback-port.md) for implementation
details, limitations, validation status, and the next acceptance gates. The
[upstream GTX fallback PR #2203](https://github.com/Marvisak/VIctory/pull/2203)
is a useful reference but is a separate, newer code line and is not merged
wholesale into this repository.

## License

GPL-3.0-or-later (see `LICENSE`). Based on VIctory (Marvisak) and victory-fork (fxceless).

