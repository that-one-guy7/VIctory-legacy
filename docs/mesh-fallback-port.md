# Mesh-shader fallback: port plan and status

**Status: experimental shader emitter added; GPU pipeline/dispatch integration and title validation remain incomplete.**

## Why this work is needed

The current host pipeline path creates a D3D12 mesh pipeline for a translated
Xbox mesh shader and records it with
ID3D12GraphicsCommandList6::DispatchMesh. The host capability check requires
both a non-zero mesh-shader tier and Shader Model 6.6. A GPU without mesh
shaders is therefore refused at pipeline creation; skipping that check alone
would not make its mesh draws work.

The NVIDIA GTX 1050 Ti is a Pascal GPU without D3D12 mesh-shader support. It
cannot execute the current native mesh path. The intended workaround is a
real conversion, not a capability override.

## Upstream research

Two upstream changes are useful references:

- [Marvisak/VIctory PR #961](https://github.com/Marvisak/VIctory/pull/961)
  describes translating each mesh shader into a compute pass that writes
  vertex/index/output buffers, followed by a generated vertex shader and a
  conventional draw. Its synthetic tests report matching output against the
  mesh path on WARP and testing on a GTX 1060. The PR's own open-items section
  says end-to-end game rendering and performance were not yet established.
- [Marvisak/VIctory PR #2203](https://github.com/Marvisak/VIctory/pull/2203)
  is a draft port of a related compute/vertex path onto a newer upstream
  snapshot. It documents additional binding, memory, indirect-draw and shader
  validation work, but does not claim verified correct world rendering or
  playable frame rates.

This repository is a separate source snapshot rather than a Git descendant of
those upstream revisions. In particular, the GPU host implementation differs
substantially, so copying either patch as a whole is not a safe mechanical
cherry-pick. Port each layer against this tree and preserve its existing
memory, descriptors, command-list, and shader-cache behavior.

## Milestone 1: workload inventory

The victory mesh-census command walks the user-supplied paths (or the
configured game/test paths) and reports aggregate native mesh-program facts:

- thread-group sizes;
- console wave size (wave32 or wave64);
- whether a program uses LDS, and the largest LDS allocation per group;
- declared maximum vertex and primitive outputs.

Use it on the user's own game directory:

~~~powershell
cargo run --release -p victory -- mesh-census "<game folder>"
~~~

Add --list to print one metadata line per discovered mesh program. The
command does not print shader code, save extracted assets, validate a host
pipeline, or prove that a fallback can run. It only sees shader containers
the existing scanner can reach (raw DXBC and supported, unpacked SGD5 records);
encrypted or otherwise unsupported archives are outside its coverage.

## Milestone 2: shader-side compute conversion (experimental)

The experimental shader-emission branch adds:

- a MeshFallback result with a compute shader, generated vertex shader,
  output layout, and a flag for per-primitive-output expansion;
- compute-mode lowering of the existing mesh program, retaining the normal
  instruction translator while redirecting vertex, index and per-primitive
  output records to three raw UAV buffers in register space 2;
- a generated vertex shader that reads those records and restores the mesh
  output signature for the existing pixel shader;
- moved user-data, root-view and sampler register spaces for the compute half
  so they do not overlap the pixel shader's bindings.

This is a shader-emission API only. The normal translate() and emit() paths
remain native and unchanged by default. The current GPU pipeline builder does
not yet call this API, allocate its scratch buffers, bind both passes, insert
barriers, or issue fallback draws. Do not use this branch as a GTX 1050 Ti
playable build yet.

## Milestone 3: direct D3D12 compute/vertex dispatch (experimental)

The direct path now has the first GPU-side integration:

- The host selects the fallback automatically when native mesh shaders are
  unavailable. `VICTORY_MESH_FALLBACK=1` can force the path on a supported
  GPU for development tests.
- Mesh pipelines contain a compute PSO for the translated producer and a
  graphics PSO using the generated vertex shader plus the original pixel
  shader and graphics state.
- The compute half's shifted root-binding spaces are represented in the
  combined root signature. Three UAV root descriptors feed the output buffers
  and three vertex-stage SRV descriptors consume them.
- Each command list has bounded, reusable device-local scratch storage. A
  direct mesh dispatch runs compute, transitions the outputs for vertex/index
  reads, draws triangles, then transitions the buffers back for the next use.
- Buffer sizing uses checked arithmetic. A single scratch allocation is
  capped at 256 MiB; allocation failure drops the draw instead of requesting
  an unbounded amount of VRAM.
- Float signature outputs are bit-cast to dwords before raw UAV stores.
- The current emitter draws the declared maximum output slots. It initializes
  unused primitive index records to zero but does not yet preserve dynamic
  per-dispatch output counts, so this can add work and still needs equivalence
  testing on shaders that emit fewer than their declared maxima.

This is **direct-dispatch support only**. Indirect mesh dispatch is explicitly
rejected until its argument conversion, count handling and indirect command
signatures are ported. The branch has not yet passed CI or a real GPU runtime
test, so do not treat it as a playable GTX 1050 Ti build.

## Milestone 4: indirect mesh dispatch conversion (draft PR #6)

The indirect path first uses the existing clamp pass to bound the title's
argument records and apply the optional count buffer. A new Shader Model 5.1
compute pass then reads those bounded records and writes two command streams:

- compute commands: three output UAV addresses plus a `Dispatch`;
- draw commands: the generated vertex SRV plus `Draw` or `DrawIndexed`.

The fallback executes the compute command signature, transitions the output
region for vertex/index reads, executes the draw command signature, then
returns both scratch pools to UAV state. A separate command pool prevents
argument-command storage from colliding with mesh output data.

**Scope limits:**
- The current direct-dispatch wrapper receives only the x group count and
  records the fallback dispatch as `x, 1, 1`.
- The indirect converter accepts only one-dimensional records (`x, 1, 1`).
  Records with y or z values other than 1 are zeroed rather than misindexed.
- Each batch is limited to 65,535 records and a 32 MiB output region. Records
  that exceed the available group capacity are converted to zero-work commands.
- The existing clamp applies optional count buffers before conversion.
- The converter HLSL has a Shader Model 5.1 compile test; command strides and
  output-region sizing have unit tests. These tests do not prove rendering
  equivalence.

This remains draft code pending Windows CI and physical-GPU testing. It is not
a playable GTX 1050 Ti build.

## Wave-size handling

The existing lowerer uses the host's maximum wave size and can adapt certain
console wave64 patterns to 32-lane hosts when cross-lane operations remain
within a half-wave and LDS constraints permit it. Mesh programs that still
require a 64-lane host wave are refused by the fallback. This needs additional
coverage against real title shaders.

## Port sequence

1. **Compile and validate:** run the Windows workspace Clippy and test jobs.
   Fix all compiler, lint and test failures before adding more GPU behavior.
2. **Shader output tests:** compare generated compute-buffer records with the
   original mesh outputs, including floats, per-primitive outputs, bounds,
   culling and multiple groups.
3. **Direct GPU test:** run on WARP and a non-mesh NVIDIA GPU with D3D12
   validation. Capture a known mesh draw and compare the generated result.
4. **Dispatch dimensions:** preserve y/z group counts for direct and indirect
   calls by passing dimensions into compute lowering and linearizing output
   offsets. The current draft only supports `x, 1, 1`.
5. **Indirect dispatch:** extend optional count-buffer coverage and verify
   command ordering and output capacity.
6. **Wave behavior:** expand tests for wave64-to-wave32 adaptation and confirm
   unsupported 64-lane programs are refused.
7. **Title validation:** run on the target game, collect DRED/device-removal
   logs, frame captures and memory usage, then verify visually correct scene
   rendering and report measured frame rate.
8. **Performance:** tune the scratch pool and output layout only after output
   equivalence is established; avoid sacrificing correctness for allocation
   speed.

## Current limitations

- The direct compute/vertex path is wired but experimental; it has not yet
  passed this fork's GPU runtime validation.
- Direct fallback currently handles only x-axis group counts. Indirect mesh
  fallback is limited to one-dimensional records, 65,535 records per batch
  and a 32 MiB output region; excess work is zeroed.
- Dynamic output-count equivalence is still unimplemented: the current path
  draws declared maxima and relies on zeroed unused primitive index records.
- Synthetic output-content equivalence tests are still needed for output
  packing, culling, wave64 and per-primitive output semantics. The converter
  compile/layout tests are not rendering-equivalence tests.
- The GTX 1050 Ti has 4 GB of VRAM. Even a correct fallback may be limited by
  temporary output buffers, the game's broader memory footprint, CPU shader
  translation costs and the 256 MiB per-allocation ceiling.
- Upstream synthetic tests do not guarantee the target title will render
  correctly or perform acceptably on this machine.


## Next acceptance gate

Before enabling the fallback by default, require all of the following:

- the generated compute output matches the mesh shader's output for synthetic
  tests, including per-primitive records;
- direct and indirect draws have matching results and correct ordering;
- no host mesh-shader interface is called on a device without that feature;
- D3D12 validation and repeat runs show no device removal or out-of-bounds
  scratch access;
- the target game reaches a captured, visually checked scene on the GTX
  1050 Ti, with frame rate and memory usage reported honestly.
