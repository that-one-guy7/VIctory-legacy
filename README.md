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

Diagnostics build: `cargo build --profile diag -p victory --features diagnostics`. When that executable is launched through `scripts\\play.ps1`, DRED is enabled automatically unless an explicit `VICTORY_D3D12_DRED` process value is already present. Release builds are unchanged.

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

When `VICTORY_MESH_FALLBACK=1` is set, `scripts/play.ps1` enables a stricter
low-VRAM profile by default: title mode `1280x720`, no extra GPU-local
title-buffer shadows, native host render/depth target dimensions
(`VICTORY_RENDER_SCALE=1.0`), two dropped top mip levels on eligible large
sampled textures, a 128 MiB aggregate local mesh-scratch cap, a 128 MiB idle
shared-scratch reuse cache, and a 70% local-memory threshold for optional UAV
optimizations. Native render-target dimensions are intentional: the GTX 1050 Ti
log recorded a `CopyResource` skipped because source and destination host
render-target scales differed. Shared scratch chunks are recycled across host
lists after their submissions finish; chunks beyond the cache limit are
released instead of being retained by hundreds of otherwise idle lists.
Active per-submission scratch can still exceed that cache limit. New scratch
beyond the local cap uses CPU-visible shared memory, which is slower but
preserves the required scratch path. Explicit environment overrides are
preserved. Texture mip dropping still reduces eligible sampled textures.
These changes can reduce image quality and performance, do not cap every
D3D12 allocation, and do not guarantee correct rendering or playable
performance.

See [docs/local-uav-memory-headroom.md](docs/local-uav-memory-headroom.md)
for the stricter profile and its allocation limits.

See [docs/mesh-fallback-port.md](docs/mesh-fallback-port.md) for implementation
details, limitations, validation status, and the next acceptance gates. The
[upstream GTX fallback PR #2203](https://github.com/Marvisak/VIctory/pull/2203)
is a useful reference but is a separate, newer code line and is not merged
wholesale into this repository.

### Latest GTX 1050 Ti diagnostic (2026-11-26, follow-up run)

The follow-up run no longer logged the mixed-render-target-scale CopyResource
skip, so the small scaled-target rectangle from the previous run is addressed.
The 3D transition is still not rendering reliably: the queue logged 36 black
frames, and the title's descriptor heap request (1,245,184 views) exceeded the
NVIDIA host heap limit used here (1,000,000). Views beyond that cap are mapped
to a null descriptor; this is a concrete rendering limitation, although the log
does not prove that every black frame came from an out-of-range descriptor.

The shader translator also refused several programs, and their draws were
dropped: one compute shader uses v_permlanex16_b32; another uses a lane-mask
exchange outside a whole-wave execution region; one compute shader reads a
descriptor's address/format as data; and two pixel shaders read descriptors
only as data. The log additionally shows a large pipeline backlog. These are
separate correctness/coverage gaps; they are not fixed by lowering the VRAM
budget. The ray-tracing structure builder remains disabled.

The final memory sample is materially different from the earlier device-removed
run:

- DXGI local GPU usage: about **3,553 MiB** against a **3,403 MiB** budget.
- GPU-shared non-local memory in use: about **6,789 MiB**.
- Physical RAM free: about **1,125 MiB**; process private bytes: about
  **17,896 MiB**.
- Commit headroom: about **44,177 MiB**. This is page-file/commit headroom, not
  physical RAM available to an active GPU allocation.
- Host copies of title memory: about **5,272 MiB**, including about
  **4,284 MiB** of windows outside title buffers.
- Host textures: about **2,994 MiB**, of which about **2,579 MiB** were
  host render/write textures.
- Mesh scratch was substantially smaller than those two pools and was not the
  dominant source of retained memory in this run.

The last failure was ID3D12GraphicsCommandList::Close returning
0x8007000E ("not enough memory resources"), followed by exit code
-2005270523. It did not end with the device-removed code from the previous
run. The error text says a recorded command was invalid; a separate
VICTORY_D3D12_DEBUG=1 run is needed to identify which command. CPU-side
refresh/copy work was also heavy: sampled frames spent around 0.3–1 second per
frame and checked multiple GiB of title-memory ranges. This is consistent with
the CPU saturation and low free-RAM reading in Task Manager.

The count of 278 host lists that had never been reset is not, by itself, proof
of a resource leak: the code's counter includes title-created host lists that
have not been used/reset. They must not be forcibly reset while the title may
still use them.

For the next crash-focused diagnostics run, `scripts/play.ps1` now enables
`VICTORY_D3D12_DEBUG=1` automatically when using the `diag` profile with
`VICTORY_MESH_FALLBACK=1` (unless an explicit process value is already set).
This activates the D3D12 debug layer and records its first validation error
when a command list fails to close. GPU-based validation
(`VICTORY_D3D12_GBV`) remains opt-in because of its substantially higher
overhead; the in-place memory probe remains read-only and
`VICTORY_INPLACE` stays disabled. This change improves the evidence for the
failure but does **not** claim the offending command has already been fixed.
The next log should include the specific D3D12 validation message needed to
select a safe code fix.

For the next diagnostics build, the low-VRAM launcher now sets
VICTORY_INPLACE_PROBE=2 by default (unless explicitly overridden). The
probe tests whether this driver can expose eligible title-owned private memory
directly to D3D12, and records memory eligibility/census results at 30 and 150
seconds. It does not enable VICTORY_INPLACE; that option pins title-owned
pages and can change memory lifetime, so it should only be enabled after the
probe shows that the driver supports it and enough memory is eligible.

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

### Mesh-scratch local-memory cap (2026-11-26)

The mesh fallback's 64 MiB scratch limit used to apply separately to each
command-list pool. Hundreds of live lists could therefore retain several
gigabytes of default-heap UAV scratch. New chunks share one process-wide
local-memory allowance (15% of DXGI's local budget, capped at 512 MiB by
default), and new allocations stop using local VRAM when current DXGI usage
reaches 80% of budget. Once either limit is reached, new mesh scratch uses a
CPU-visible shared-memory UAV buffer. This can reduce performance but avoids
adding more local allocations. `VICTORY_MESH_LOCAL_BUDGET_MB` may lower the
automatic allowance; it cannot raise it.
The new diagnostics build also reports live mesh scratch separately:
local default-heap chunks and CPU-visible shared-memory fallback chunks. This
will distinguish retained mesh-pool memory from the cumulative "made so far"
buffer counters in the next hardware log.

## License

GPL-3.0-or-later (see `LICENSE`). Based on VIctory (Marvisak) and victory-fork (fxceless).

