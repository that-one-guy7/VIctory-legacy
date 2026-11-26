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

## Experimental render-resolution scaling

`VICTORY_RENDER_SCALE=0.5` reduces eligible host-side 2D render/depth targets to half width and height. The default is `1.0` (off). The default minimum original dimension is 512 pixels; override it with `VICTORY_RENDER_SCALE_MIN_SIZE`. This is an experimental memory/performance trade-off, not a general game graphics setting. See [docs/render-resolution-scale.md](docs/render-resolution-scale.md) for eligibility rules, limitations and the test procedure. Keep it disabled unless you are testing this behavior.

## Diagnostic log

The launcher records effective `VICTORY_*` settings, Windows/PowerShell versions, and best-effort CPU, display-adapter, and physical-memory details in `%TEMP%\\ztrx.log`. This makes stale environment overrides and machine differences easier to spot without dumping unrelated process variables. See [docs/diagnostic-log.md](docs/diagnostic-log.md) before sharing a log.

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

When `VICTORY_MESH_FALLBACK=1` is set, `scripts/play.ps1` defaults the title's
reported render mode to `1280x720` if `VICTORY_DISPLAY_MODE` is unset, disables
extra GPU-local title-buffer shadows (`VICTORY_CARD_BUFFERS=0`) to leave more
VRAM for textures and mesh scratch, and enables parallel CPU refresh, snapshot
and merge work plus batched GPU hash passes. Explicit environment overrides
are preserved. Disabling the extra GPU-local copies favors stability over
their faster memory path; this is still an experimental profile, not a
guarantee of correct rendering or playable performance.

See [docs/mesh-fallback-port.md](docs/mesh-fallback-port.md) for implementation
details, limitations, validation status, and the next acceptance gates. The
[upstream GTX fallback PR #2203](https://github.com/Marvisak/VIctory/pull/2203)
is a useful reference but is a separate, newer code line and is not merged
wholesale into this repository.

### Latest GTX 1050 Ti diagnostic (2026-11-26)

The newest `ztrx(10).log` did not reach a visible 3D scene. It ended at
frame 2294 when a D3D12 command-list prelude failed with
`0x8007000E`. The final detailed sample showed 18.0 GB private process
memory, 446 MB free system RAM, 2.72 GB of host textures and 3.13 GB of
card buffers. The submitting thread was busy for 0.51–0.94 seconds in
sampled frames while the GPU was only about 21–39% busy. This points to
memory pressure and CPU submission overhead, not a GPU that is simply
underpowered at rendering.

The same log shows 522 card buffers alive, so the launcher’s
`VICTORY_CARD_BUFFERS=0` default was not confirmed active; the launcher
preserves explicit environment overrides. It now writes the effective
settings into `%TEMP%\ztrx.log`, and the D3D12 prelude failure now reports
whether allocator creation, command-list creation or list close failed.

Upstream VIctory issue [#417](https://github.com/Marvisak/VIctory/issues/417)
reports the same `0x1c8` and `0x218` warnings. Upstream PR
[#518](https://github.com/Marvisak/VIctory/pull/518) found that returning
`S_OK` without writing the methods' output parameters crashes title worker
threads; those methods must remain unsupported until their contracts are
identified. See [the detailed log analysis and PortWatch comparison](docs/mesh-fallback-port.md).

### Compatibility-layer research

The most useful comparison is [fxceless/victory-fork](https://github.com/fxceless/victory-fork):
its README reports 8–10 FPS in the first rooms on an RTX 4070 12 GB with
32 GB system RAM, and identifies the CPU submitting thread as a bottleneck.
[XCL](https://github.com/xcl-dev/xcl) demonstrates non-blocking pipeline
creation and broad shader translation, but PortWatch still reports world-load
and GPU-watchdog issues. The best-looking AMD fork is tested on a much larger
system (RX 9070 XT 16 GB, 64 GB RAM) and is not a direct GTX 1050 Ti reference.
PortWatch's claims are a tracker of public project reports, not all
independently reproduced benchmarks.

### GPU allocation headroom guard (2026-11-26)

The 85% DXGI local-memory guard is limited to optional GPU optimizations
(card-buffer shadows/hash staging, depth twins and texture alias-copy staging).
Required mesh scratch and indirect-argument buffers retain their prior
allocation path so a budget refusal cannot silently disable core rendering
work. This is still advisory rather than a global VRAM cap; see
[docs/local-uav-memory-headroom.md](docs/local-uav-memory-headroom.md).

## License

GPL-3.0-or-later (see `LICENSE`). Based on VIctory (Marvisak) and victory-fork (fxceless).

