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

## Faster settings (experimental, off by default)

The diagnostic low-VRAM profile keeps asynchronous GPU submission and the
copy-back relay disabled while black-scene rendering is being isolated. To
reproduce the experimental PR #34 overlap path, set
`$env:VICTORY_GPU_OVERLAP="1"` in PowerShell before launching
`scripts\\play.ps1`. It defaults to `VICTORY_LATE_WAIT=3`,
`VICTORY_LATE_WAIT_LISTS=64`, and `VICTORY_RELAY=2`; explicitly supplied
values for those three settings are honored only after opting in. The
stage-3 path omits the diagnostics-only per-present black-frame readback.
`VICTORY_WIDE_BACK=1` remains a separate experimental option.

## Experimental render-resolution scaling

`VICTORY_RENDER_SCALE=0.5` reduces eligible host-side 2D render/depth targets to half width and height. The general default is `1.0` (off); the `diag` low-VRAM launcher preset clamps the value to `0.5` or lower on 4 GB-class testing systems so stale PowerShell environment values cannot silently disable the reduction. The default minimum original dimension is 512 pixels; override it with `VICTORY_RENDER_SCALE_MIN_SIZE`. To deliberately test a larger scale (including native size), set `VICTORY_RENDER_SCALE_OVERRIDE=1` and then set `VICTORY_RENDER_SCALE` to the desired value. This is an experimental memory/performance trade-off, not a general game graphics setting. See [docs/render-resolution-scale.md](docs/render-resolution-scale.md) for eligibility rules, limitations and the test procedure.

## Diagnostic log

The launcher records effective `VICTORY_*` settings, Windows/PowerShell versions, and best-effort CPU, display-adapter, and physical-memory details in `%TEMP%\\ztrx.log`. This makes stale environment overrides and machine differences easier to spot without dumping unrelated process variables. See [docs/diagnostic-log.md](docs/diagnostic-log.md) before sharing a log.

## Debugging switches

| Switch | Effect |
|---|---|
| `VICTORY_CAPTURE=<dir>` + `VICTORY_CAPTURE_FRAMES=<list>` | save those frames as PNG (diagnostics build) |
| `VICTORY_KEYS=30:Enter,35:X` | press keys at those seconds (diagnostics build) |
| `VICTORY_D3D12_DRED=1` | on a GPU failure, report the commands in flight |
| `VICTORY_PIPELINE_WAIT_MS=24` | shorter per-frame wait for pipelines still being built |
| `VICTORY_PIPELINE_JOURNAL=1` | diagnose driver crashes during host pipeline creation; see below |

Diagnostics build: `cargo build --profile diag -p victory --features diagnostics`. When that executable is launched through `scripts\play.ps1`, DRED is enabled automatically unless an explicit `VICTORY_D3D12_DRED` process value is already present. Release builds are unchanged.



### Diagnosing driver crashes during pipeline creation

Some graphics drivers can crash inside their shader compiler while VIctory
creates a host pipeline. The process can end before VIctory receives an error
code, so the ordinary log may not identify the shader. Set
`VICTORY_PIPELINE_JOURNAL=1` before launching to enable an opt-in journal
beside the host pipeline cache:

```powershell
$env:VICTORY_PIPELINE_JOURNAL = "1"
```

Each in-flight host pipeline gets a small file in
`pipeline-journal/pending`. On the next run, a single leftover note is
recorded in `pipeline-journal/crashed.txt` and that pipeline is skipped;
several notes become suspects and are retried one at a time. This can isolate
a pipeline that crashes the driver and may let the title proceed, but draws
or dispatches using a refused pipeline are dropped. It is a diagnostic
workaround, not a shader translation fix.

When the journal is enabled and the low-VRAM/diagnostic launcher profile is
active, the launcher defaults `VICTORY_PIPELINE_WORKERS` to `1` unless an
explicit process override is already set. This serializes host PSO creation
during the diagnostic run so a driver crash normally leaves one identifiable
pending pipeline rather than several concurrent candidates. Cold pipeline
creation can take longer; with the journal off, the profile keeps its existing
two-worker default.

The legacy branch keeps a manual translator stamp in
`crates/victory-gpu/src/pipeline/journal.rs`; bump it whenever changes to
`victory-shader` can change emitted DXIL. A crash list from an older stamp
then becomes suspects and is tested again. To retry one refused pipeline
manually, remove its hash line from `pipeline-journal/crashed.txt` while
keeping the translator header. Do not force-close VIctory while it is
compiling pipelines: a process stopped mid-creation leaves a note too.

The journal is off unless the variable is set. Enable it only while
investigating a repeatable pipeline-creation crash.

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

When `VICTORY_MESH_FALLBACK=1` is set, or the executable is built with the
`diag` profile, `scripts/play.ps1` enables a stricter low-VRAM profile by
default. The diagnostic profile uses this preset for the GTX 1050 Ti
crash-reproduction path because the runtime may select compute/vertex mesh
fallback automatically when native mesh shaders are unavailable; no manual
fallback flag is required. Most preset values still honour explicit process
settings. The diagnostic low-VRAM profile has three deliberate exceptions to
avoid stale values defeating the tested safety settings: eligible host render
and depth targets are scaled to at most half size unless
`VICTORY_RENDER_SCALE_OVERRIDE=1`; GPU-local shadows are enabled only for title
buffers at least 64 MiB, with a 128 MiB aggregate cap
(`VICTORY_CARD_BUFFERS=1`, `VICTORY_CARD_BUFFERS_MIN_MB=64`,
`VICTORY_CARD_BUFFERS_MB=128`), unless `VICTORY_CARD_BUFFERS_OVERRIDE=1`; and
host-list submissions are grouped in batches of two, with one fence wait per
batch. The batch size can be changed to 1–16 with `VICTORY_GPU_BATCH_LISTS`; set
`VICTORY_GPU_BATCH_LISTS_OVERRIDE=1` to prevent the diagnostic launcher from
resetting it to two. Per-list GPU timing remains disabled by default because its
fence wait after every host list significantly reduced throughput. To opt into
that detailed TDR trace, set `VICTORY_GPU_TIMING_OVERRIDE=1`; timing then takes
precedence over batching. The GPU-local buffer copies are only attempted while
adapter budget checks permit them; if the budget is too tight, the runtime falls
back to its ordinary CPU-backed copies. The preset otherwise
uses title mode `1280x720`, three dropped top mip levels on eligible large
sampled textures, indirect-window batching disabled by default
(`VICTORY_INDIRECT_WINDOW=0`), a 1 MiB grid for length-bounded title-memory
windows (`VICTORY_WINDOW_KB=1024`; unsized root views retain 4 MiB windows),
a 128 MiB aggregate local mesh-scratch cap, a 32 MiB idle shared-scratch
reuse cache, and a 70% local-memory threshold for optional UAV optimizations.
The low-VRAM profile also limits DirectStorage to 3 workers and host pipeline
compilation to 2 workers, leaving CPU headroom for the title on an 8-thread
system. These settings reduce some avoidable memory/copy overhead but cannot
cap every D3D12 allocation or guarantee a stable frame rate on a 4 GB adapter. ### Conservative LDS fixes for 32-lane hosts

Some console wave64 compute programs are refused by D3D12 adapters whose
widest wave is 32 lanes. Two deliberately narrow recovery checks are
available for controlled tests; both are **off by default**:

- `VICTORY_LDS_EXACT=1` permits a lane-branch region with LDS reads and
  writes only when static analysis can prove the operations do not need
  ordering on the same LDS dword. Unknown addresses remain conservative.
- `VICTORY_LDS_FLAGS=1` permits only a region that raises one constant LDS
  flag, has no other side effects, and leaves no live register values
  dependent on the timing of the flag read.

These checks are not general Wave64 emulation. They do not fix shaders that
depend on cross-half lane reads, lane masks, dynamic LDS addresses, or other
unsupported wave64 operations. The upstream tests that motivated them
recovered specific compute pipelines `369cbe906ea72764` and
`2cc4c197ee29e291` on other 32-lane NVIDIA GPUs; that is not a guarantee
for this build or adapter. Enable one switch at a time first and compare
both visuals and stability. The synthetic tests cover the supported
patterns, but physical GPU validation is still required.

The smaller grid aims to trim unused
tails of host copies, but can create more overlapping windows; memory and
frame-time measurements must verify the trade-off. Native render-target
dimensions are intentional: the GTX 1050 Ti
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

### GTX 1050 Ti recovery and low-VRAM follow-up (2026-11-27)

Disabling native FSPX compute handling restored visible 3D output in the latest
GTX 1050 Ti run, but it is still not a playable result. The run reached about
0.5 FPS and ended when closing a D3D12 prelude command list returned
`E_OUTOFMEMORY (0x8007000E)`. At the final sample, local GPU memory was
3,735 MiB against a 3,403 MiB budget; host textures were 3,153 MiB, including
2,702 MiB of host copies the game renders to or writes. Host copies of title
memory held another 4,565 MiB, and the PC had about 1,227 MiB physical memory
free. The adapter was over budget; this was not a page-file commit-limit error.

The low-VRAM profile now tries three dropped top mip levels on eligible large
sampled textures and enables bounded indirect-record windowing. The latter
can collapse repeated per-record validation dispatches for adjacent
one-record indirect draws; it does not batch mesh shaders themselves. These
are measured-risk settings, not a claim that the OOM is fixed. Host render
target scaling remains at 1.0: the existing scaling experiment can skip copies
when source and destination resources use different scales, so enabling it by
default could reintroduce missing lighting or other output.

The same run shows four compute pipelines refused because the GTX 1050 Ti
supports 32-lane host waves while those translated shaders request wave64.
It also records three compute translation failures for `v_cvt_f32_f64` and
one divergent lane-mask refusal. These are concrete shader-compatibility gaps
that may account for some missing lighting; the log does not identify those
four wave64 shaders as the sole source of the remaining lighting defects.
The packed-light texture alias path is active in this run, so disabling it is
not an appropriate workaround. The next wave64 investigation should record
`VICTORY_WAVE_WHY=1` and decide whether each program can be correctly
lowered across two wave32 halves; forcing wave32 indiscriminately is unsafe.

For meaningful frame-time comparisons, use a release executable. The
diagnostic executable enables the D3D12 debug layer and DRED and adds
substantial tracing and memory accounting; its 0.5 FPS is not a clean
release-performance measurement.

### Black 3D output follow-up (2026-11-26 23:10 local / -06:00)

The latest run is not a swap-chain-only failure. It recorded 935 shader
translation failures (933 compute), 16,384 calls through still-unimplemented
device slot `0x1c8`, and nine pixel-shader attempts rejected because this
adapter does not support barycentrics. The FSPX parser accounted for 491
failures with a generic unsupported-root-type error. This patch recognizes
the resource-heap and sampler-heap root kinds as one-dword heap base
parameters, matching the existing resource tracer's 32-bit address handling,
and reports the raw root-type value for any remaining unsupported kind.
Parser tests cover both supported heap kinds and an unsupported value.

The same run reached 3,706 MiB of local GPU usage against a 3,403 MiB budget,
with 740 MiB of physical memory free near the end. The D3D12 device was
removed with `DXGI_ERROR_DEVICE_HUNG` after 15 submissions / 138 lists were
in flight. The diagnostic low-VRAM profile now caps the in-flight list count
at 64 (`VICTORY_LATE_WAIT_LISTS=64`) while retaining asynchronous overlap.
This is intended to limit transient command/descriptor/resource retention;
it can introduce more waits if the GPU cannot keep up.

This is a targeted follow-up, not yet a complete black-scene fix. Slot
`0x1c8` remains unimplemented, the remaining compute translation errors
still need individual support, and barycentric-dependent pixel shaders are
still refused on adapters without that feature. No claim is made that all 3D
lighting will render until a new GTX 1050 Ti run confirms it.

### Native FSPX compute experiment and GPU overlap

The native FSPX compute handler for device slot `0x218` assembles
precompiled RDNA2 sections (packet, metadata, code and resource records) into
an `ACCC` container and sends the program through the native compute
translator. This code path is **experimental and disabled by default**.
Without `VICTORY_EXPERIMENTAL_FSPX=1`, the slot keeps its previous
`E_NOTIMPL` behavior so the title can select its fallback path. Set that
variable to `1` only for a controlled comparison.

The default-off policy is deliberate. An independent test of GTA VI
`1.0.2.1` on another VIctory branch found that enabling the slot-0x218
handler increased dropped draws from an average of 82 to 1,440 per frame.
The title was using the handler for its own compute pipelines, not just the
FidelityFX upscaler; untranslated programs caused the world to render black.
See [upstream issue #2394 / PR #2396](https://github.com/Marvisak/VIctory/pull/2396)
for the test report. The 2026-11-27 GTX 1050 Ti log likewise records 958
shader translation failures: 553 descriptors changed after loading and 207
descriptors could not be traced as loaded or built. Keeping the handler
enabled would accept these pipeline objects even though their shaders cannot
be translated, suppressing the title's fallback behavior.

Other independent blockers remain: the same log records 16,384 calls to
unimplemented device slot `0x1c8`, and 10 pixel-shader creation failures
because the GTX 1050 Ti does not support the shaders' barycentric inputs.
The default-off FSPX change does not implement either path and does not
claim that all 3D rendering is fixed.

As of the 2026-11-27 GTX 1050 Ti run, asynchronous overlap is also
**not enabled by default** in the diagnostic low-VRAM profile. The latest
run had `VICTORY_LATE_WAIT=0` and `VICTORY_RELAY=0`; it still recorded
black frames and shader translation failures. The launcher forces both
settings to zero unless `VICTORY_GPU_OVERLAP=1` is explicitly set, restoring
the per-present black-frame readback. The async path remains available for
controlled A/B testing; it can keep more work in flight and stage 3 omits the
diagnostics-only per-present black-frame readback.

### Follow-up GTX 1050 Ti crash run before PR #32 (2026-11-26, 21:54 local / -06:00)

This run was built before PR #32 was merged. Its final memory report still says
`windows outside buffers (grid 4096 KiB)`, so it is not a validation run for
the 1 MiB window setting. It ended with an unhandled access violation
`0xC0000005` in `nvwgf2umx.dll+0x159f58` (read from address `0xC`), not the
D3D12 `E_OUTOFMEMORY` seen in the earlier run. The final GPU memory sample
reported 3,890 MiB against a 3,403 MiB local budget and 6,443 MiB of shared
system memory in use. Windows reported 1,315 MiB of physical RAM free; the
process had 21,774 MiB of private bytes. Host textures held 3,273 MiB,
title-memory host copies held 4,344 MiB, and mesh fallback held 1,181 MiB in
shared buffers (103 MiB of that was idle spare) plus 128 MiB local. The
DirectStorage pool started 8 workers on the machine's 8 logical processors.
These figures motivate tighter background-worker and idle-cache defaults;
they do not prove the NVIDIA driver access violation has a single cause.

See [docs/local-uav-memory-headroom.md](docs/local-uav-memory-headroom.md)
for the stricter profile and its allocation limits.

See [docs/mesh-fallback-port.md](docs/mesh-fallback-port.md) for implementation
details, limitations, validation status, and the next acceptance gates. The
[upstream GTX fallback PR #2203](https://github.com/Marvisak/VIctory/pull/2203)
is a useful reference but is a separate, newer code line and is not merged
wholesale into this repository.

### Latest GTX 1050 Ti crash run (2026-11-26, 20:56 local / -06:00)

The 2026-11-26 20:56 local run (-06:00) reached frame 2,876, then failed while recording a host texture-state
prelude with D3D12 `E_OUTOFMEMORY (0x8007000E)`; the launcher exited with
`0x887A0005`. The GPU-budget pressure handler did activate and texture staging
had fallen to 24 MiB, so the previous residency guard was running but did not
cover the larger remaining pools.

The final memory sample's GPU-budget line reported 4,214 MiB in use
against a 3,403 MiB budget; 6,198 MiB of PC memory was shared with the GPU,
and 1,567 MiB of physical RAM remained free. Host textures were about
2,860 MiB. Host copies of title memory were 4,557 MiB, including 3,568 MiB
across 854 windows outside title buffers; the diagnostic line's low-to-high
asked-address spans within those windows totaled about 1,694 MiB. Mesh
fallback scratch held 477 MiB in local buffers and 739 MiB in shared-memory
buffers, including 236 MiB cached spare. The memory report also counted
1,040 MiB of card buffers made since startup; this is cumulative allocation,
not a live-byte count.

The same memory report shows a host-copy sizing opportunity. Its 854 live
windows outside title buffers held 3,568 MiB, while the reported spans from
the lowest to highest asked address within those windows totaled about
1,694 MiB. The implementation defaults to a 4 MiB grid for length-bounded
windows and documents 1 MiB as a way to end windows nearer the requested
ranges. The low-VRAM launcher now uses 1 MiB for those length-bounded windows;
the 432 asks without a length (about 1,728 MiB in this run) remain on the
fixed 4 MiB root-view grid. A smaller grid can create more overlapping windows
and duplicate copies, so this is a measured trade-off to verify in the next
run, not a guaranteed memory reduction.

The launcher header showed that no low-VRAM profile overrides or
`VICTORY_D3D12_DEBUG` setting were active: only DRED was enabled. The runtime
nevertheless used mesh fallback automatically. The old launcher nested both
the low-VRAM preset and the debug-layer default under the explicit
`VICTORY_MESH_FALLBACK=1` flag, so automatic fallback skipped both. The
diagnostic launcher now applies the low-VRAM defaults to `diag` builds and
enables the D3D12 debug layer on every `diag` launch. Explicit settings still
win, and release launches remain unchanged unless the fallback flag is set.

The host-prelude failure now names whether allocator reset, command-list reset,
or list close failed, and reports the number of pre-copy barriers, final
barriers, and texture-twin copies for context. Barrier and copy recording APIs
return no error, so they are not reported as separate failure stages. The next
run can therefore distinguish the failing D3D12 operation and include the
debug layer's first validation message. This is a targeted mitigation and
diagnostic improvement, not a claim that this particular OOM is fixed.

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

Every `diag` launch now enables `VICTORY_D3D12_DEBUG=1` automatically
(unless an explicit process value is already set), independently of whether
mesh fallback was requested manually or selected automatically by the runtime.
This activates the D3D12 debug layer and records its first validation error
when a command list fails to close. GPU-based validation
(`VICTORY_D3D12_GBV`) remains opt-in because of its substantially higher
overhead; the in-place memory probe remains read-only and
`VICTORY_INPLACE` stays disabled. The launcher also applies the low-VRAM
preset on `diag` builds, without forcing mesh fallback. The new host-prelude
failure report names the exact recording step and counts its barriers and
texture-twin copies. These changes improve the evidence and reduce expected
VRAM pressure; they do **not** claim that the latest OOM is already resolved.

The in-place memory probe remains opt-in for crash-reproduction runs:
its periodic census and temporary heap-opening test add extra work and can
temporarily keep title pages resident. VICTORY_INPLACE remains disabled.
Only enable VICTORY_INPLACE_PROBE in a separate experiment when measuring
whether this driver can expose eligible title-owned memory directly to D3D12.

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

