# Mesh-shader fallback: port plan and status

**Status: investigation and workload census; compute/vertex fallback not yet implemented.**

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

## First milestone: inventory the workload

The new victory mesh-census command walks the user-supplied paths (or the
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
pipeline, or prove that a fallback can run. It also only sees shader
containers the existing scanner can reach (raw DXBC and supported, unpacked
SGD5 records); encrypted or otherwise unsupported archives are outside its
coverage.

## Port sequence

1. **Census:** collect the actual mesh group sizes, wave64/LDS use, output
   limits and translation refusal reasons from the target title. Do not assume
   every mesh program has the same shape.
2. **Shader lowering:** introduce an explicit fallback result containing a
   compute shader, a generated vertex shader, and a precise output-buffer
   layout. Preserve user data, root views, descriptor heap accesses, pixel
   semantics, per-primitive outputs, vertex/primitive bounds and group IDs.
3. **Pipeline creation:** build the compute and vertex/pixel PSOs only when
   native mesh shaders are unavailable or the caller explicitly requests the
   fallback. Keep the existing native path unchanged on supported GPUs.
4. **Command recording:** allocate bounded per-list scratch buffers, dispatch
   compute, insert the required UAV/transition barriers, then issue indexed
   or expanded draws. Handle zero groups and maximum output sizes safely.
5. **Indirect dispatch:** translate console mesh-dispatch argument records
   into compute dispatch and draw arguments without losing count-buffer or
   ordering semantics.
6. **Wave behavior:** verify console wave64 operations on 32-lane NVIDIA
   execution groups. A mesh-to-compute conversion does not automatically
   solve wave64 emulation.
7. **Validation:** add synthetic buffer-content and image-equivalence tests,
   run workspace Clippy/tests, then test on a non-mesh GPU. Report the exact
   adapter, driver, Agility runtime, VRAM/RAM, settings, frame captures and
   failure logs. Do not call the game playable based on pipeline creation or
   non-black frames alone.

## Current limitations

- No compute/vertex fallback is present in this repository yet.
- The census is a diagnostic milestone, not compatibility support.
- The GTX 1050 Ti has 4 GB of VRAM; even a correct fallback may be limited by
  temporary output buffers, the game's broader memory footprint, shader-model
  features and CPU/GPU translation costs.
- The upstream synthetic-test results do not guarantee the target title will
  render correctly or perform acceptably on this machine.

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
