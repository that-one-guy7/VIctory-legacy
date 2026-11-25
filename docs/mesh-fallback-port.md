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

## Milestone 3: direct-dispatch GPU integration (draft PR #3)

The direct-dispatch draft adds the first renderer-side integration:

- automatic selection on adapters without native mesh shaders, plus
  `VICTORY_MESH_FALLBACK=1` to force the path for comparison;
- compute and graphics pipeline states sharing a root signature, with
  compute-stage user data/root views shifted away from pixel-stage bindings;
- three UAV outputs and three generated-vertex SRVs in the reserved output
  register space;
- per-command-list default-heap scratch buffers, with explicit transitions
  from COMMON to UAV, UAV to shader/index read, and back to UAV;
- a direct compute dispatch followed by an indexed triangle draw, or an
  expanded non-indexed triangle draw when per-primitive outputs are present.

The draft explicitly refuses non-triangle mesh topologies. It does not yet
handle indirect mesh dispatches. CI and physical-GPU validation are pending;
there are no synthetic output-equivalence tests yet. This is not a playable
build and should not be tested against the game until the PR is validated and
merged.

## Milestone 4: indirect dispatch conversion (draft PR #4)

The indirect path now has a separate argument-conversion compute pass. The
existing indirect clamp first bounds the title's argument records; the new
pass reads those bounded records and writes two command streams:

- compute commands: three output UAV addresses plus a `Dispatch`;
- draw commands: the generated vertex-buffer SRV plus `Draw` or
  `DrawIndexed`.

The fallback then executes the compute command signature, transitions the
output region for vertex/index reads, executes the draw command signature, and
returns the scratch pools to UAV state. A separate per-list command scratch
pool is used. The command-converter HLSL is compiled as Shader Model 5.1 and
has a compile test plus layout/stride unit tests.

**Current scope limits:**
- Only one-dimensional mesh dispatch records (`x, 1, 1`) are accepted.
  Records with y or z group counts other than 1 are converted to zero-work
  commands rather than misindexed.
- Each indirect batch is limited to 65,535 records and a 32 MiB output region.
  The converter zeros records that would exceed the batch's group capacity.
- This is still draft code: CI and physical-GPU validation are pending.
  No output-equivalence or target-game rendering test has passed yet.

## Wave-size handling

The existing shader lowerer uses the host's maximum wave size. It can map
certain console wave64 patterns onto 32-lane host waves when the cross-lane
operations remain within a half-wave and the LDS constraints permit it.
Patterns that still require 64 lanes remain unsupported and are refused by
the mesh fallback rather than being sent to a host that cannot execute them.
This behavior needs more coverage tests against real title shaders.

## Port sequence

1. **Census:** collect the actual mesh group sizes, wave64/LDS use, output
   limits and translation refusal reasons from the target title. Do not assume
   every mesh program has the same shape.
2. **Shader lowering:** complete synthetic tests for output buffer contents,
   signatures, resource bindings and per-primitive outputs.
3. **Direct dispatch:** the draft PR builds compute and vertex/pixel PSOs,
   allocates bounded per-list scratch buffers, inserts resource barriers and
   issues indexed or expanded triangle draws. This still needs runtime testing.
4. **Indirect dispatch:** translate console mesh-dispatch argument records
   into compute dispatch and draw arguments without losing count-buffer or
   ordering semantics.
5. **Wave behavior:** expand tests for the existing wave64-to-wave32 lowering,
   and confirm that unsupported 64-lane patterns are refused.
6. **Validation:** add synthetic buffer/image equivalence tests, run DXIL
   validation and workspace Clippy/tests, then test on a non-mesh GPU. Report the
   exact adapter, driver, Agility runtime, VRAM/RAM, settings, frame captures
   and failure logs. Do not call the game playable based on pipeline creation
   or non-black frames alone.

## Current limitations

- The direct-dispatch GPU path is present only in draft PR #3; CI and physical
  GPU validation are pending.
- Indirect fallback is limited to one-dimensional records, 65,535 records per
  batch, and a 32 MiB output region; excess work is zeroed.
- No synthetic output-content/image-equivalence or physical GPU test has
  passed on this repository's snapshot.
- Some mesh shaders still require wave64 and are refused on 32-lane hosts.
- The census is a diagnostic milestone, not compatibility support.
- The GTX 1050 Ti has 4 GB of VRAM; even a correct fallback may be limited by
  temporary output buffers, the game's broader memory footprint, shader-model
  features and CPU/GPU translation costs.
- Upstream synthetic-test results do not guarantee the target title will
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

## Follow-up correctness finding: group-local indices

A conventional draw combines output from multiple mesh workgroups, while the
mesh shader's index values are local to one workgroup. Treating those values
as global vertex indices makes later groups read the first group's vertices.
The fallback now handles this separately for both draw modes:

- Indexed mode adds `groupId.x * max_vertices` when the compute shader stores
  each group's indices. Indirect indexed draws additionally set
  `BaseVertexLocation` to the cumulative group base for that record.
- Expanded mode leaves the stored local indices unchanged and has the
  generated vertex shader derive the absolute group base from
  `SV_VertexID / (max_primitives * 3)`. This also works when an indirect
  draw starts at a later record's `StartVertexLocation`.

The indirect argument converter now zeros records whose x group count exceeds
D3D12's 65,535 per-dimension dispatch limit. The direct fallback refuses a
larger count before recording a dispatch.

These are correctness fixes, not proof of output equivalence. The next gate is
DXIL validation and synthetic output tests, followed by D3D12 validation on a
non-mesh GPU. The GTX 1050 Ti remains unverified on physical hardware.
