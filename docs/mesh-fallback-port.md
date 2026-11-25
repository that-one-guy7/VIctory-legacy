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

This is **direct-dispatch support only**. Indirect mesh dispatch is explicitly
rejected until its argument conversion, count handling and indirect command
signatures are ported. The branch has not yet passed CI or a real GPU runtime
test, so do not treat it as a playable GTX 1050 Ti build.

## Port sequence

1. **Compile and validate:** run the Windows workspace Clippy and test jobs.
   Fix all compiler, lint and test failures before adding more GPU behavior.
2. **Shader output tests:** compare generated compute-buffer records with the
   original mesh outputs, including floats, per-primitive outputs, bounds,
   culling and multiple groups.
3. **Direct GPU test:** run on WARP and a non-mesh NVIDIA GPU with D3D12
   validation. Capture a known mesh draw and compare the generated result.
4. **Indirect dispatch:** translate console mesh-dispatch argument records,
   optional count buffers and draw records while preserving ordering.
5. **Wave behavior:** verify console wave64 operations on 32-lane NVIDIA
   execution groups. Mesh-to-compute conversion does not automatically solve
   wave64 emulation.
6. **Title validation:** run on the target game, collect DRED/device-removal
   logs, frame captures and memory usage, then verify visually correct scene
   rendering and report measured frame rate.
7. **Performance:** tune the scratch pool and output layout only after output
   equivalence is established; avoid sacrificing correctness for allocation
   speed.

## Current limitations

- The direct compute/vertex path is wired but experimental; it has not yet
  passed this fork's GPU runtime validation.
- Indirect mesh dispatch is deliberately rejected rather than sending a
  native mesh-dispatch command to a device without that capability.
- Synthetic output-content equivalence tests are still needed for output
  packing, culling, wave64 and per-primitive output semantics.
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
