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

The GTX 1050 Ti cannot use the native D3D12 mesh-shader pipeline. This branch
adds an experimental compute/vertex replacement that is selected automatically
when the host lacks native mesh-shader support. It does not disable the
capability check or call `DispatchMesh` on unsupported hardware.

The implementation includes:
- mesh-to-compute shader emission and a generated vertex shader;
- a direct compute dispatch followed by a conventional indexed or expanded
  triangle draw;
- bounded conversion of indirect mesh-dispatch records into compute and draw
  command streams; and
- fallback-specific scratch buffers, root bindings and resource transitions.

A correctness review also found that mesh output indices are local to each
dispatch group. This branch rebases them for indexed draws, derives group
offsets from `SV_VertexID` for expanded draws (including indirect batches),
and carries the cumulative base vertex in indexed indirect draw arguments.
Indirect dispatch counts above D3D12's 65,535-per-dimension limit are rejected.

**The fallback is experimental and is not yet validated as playable on the
GTX 1050 Ti.** The next acceptance gates are shader-output equivalence tests,
DXIL/D3D12 validation, and a physical GPU run. Some wave64 programs may still
be refused on 32-lane hardware. The fallback also requires Shader Model 6.6
and Resource Binding Tier 3 for the renderer's directly indexed descriptor
heaps; startup now queries both capabilities and refuses the fallback if the
adapter lacks either. The GTX 1050 Ti's 4 GB VRAM may also constrain temporary
output buffers and the game's overall memory use.

Run the read-only workload inventory with:

```powershell
cargo run --release -p victory -- mesh-census "<game folder>" --list
```

See [docs/mesh-fallback-port.md](docs/mesh-fallback-port.md) for implementation
details, known limits and acceptance criteria.

## License

GPL-3.0-or-later (see `LICENSE`). Based on VIctory (Marvisak) and victory-fork (fxceless).

### Follow-up correctness work (2026-11-25)

The fallback review found that mesh output indices are local to each dispatch
group. A single conventional draw concatenates all groups, so those indices
must be rebased. This branch now:
- offsets indices for indexed direct dispatches by the group's vertex base;
- derives the group base from `SV_VertexID` for expanded per-primitive draws,
  including later indirect records;
- adds the cumulative vertex base to indexed indirect draw arguments; and
- rejects indirect compute dispatch records above D3D12's 65,535-group limit.

These changes correct an output-addressing issue in the experimental fallback;
they do **not** establish pixel-equivalent rendering or GTX 1050 Ti runtime
support. Physical GPU validation and the device's shader-model/resource-binding
capabilities remain acceptance gates.
