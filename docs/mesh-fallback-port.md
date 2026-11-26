# Mesh-shader fallback: port plan and status

**Status: experimental compute/vertex fallback is wired for direct and indirect dispatch. Windows CI run [45731295379](https://github.com/that-one-guy7/VIctory-legacy/actions/runs/45731295379) passed Clippy, the diagnostics build, workspace tests, Agility SDK staging, the Release build, export checks, and PowerShell launcher parsing for commit `ca6a1e8`. That follow-up fixes the shifted mesh-compute descriptor-table register space and disables extra GPU-local title-buffer shadows by default in the low-VRAM fallback profile. Physical-GPU/image-equivalence validation remains outstanding.**

## Follow-up from the GTX 1050 Ti runtime log

The 2026-11-26 diagnostic run reached the 1280x720 title path, but D3D12
rejected many mesh-fallback compute PSOs with
`0x80070057`. The debug InfoQueue identified the exact mismatch: translated
mesh-compute shaders requested root SRVs in register space 9, while bounded
descriptor-table ranges were still declared in register space 1. The root
descriptors, user data and samplers already applied the mesh-compute shift;
the table-range path did not. The range builder now applies the same
`ROOT_CBV_SPACE + shift` rule, with a unit test for ordinary and shifted SRV
and UAV ranges.

The same run also reported a 32 MiB unordered-access buffer allocation failure
(`0x8007000E`) after GPU memory use had climbed substantially. A first-chance
access violation was then reported inside the D3D12 debug-layer DLL while the
title was reporting its crash; the log does not prove that this exception was
the original cause of the title failure. For the fallback launcher profile,
large title-buffer copies are now kept in CPU-visible memory by default
(`VICTORY_CARD_BUFFERS=0`), avoiding those additional GPU-local copies that
compete with texture resources and mesh scratch. An explicit user setting
still overrides this fallback default. This favors stability over the faster
GPU-local buffer-copy path and may reduce performance.

The next physical run should use the diagnostics build with the D3D12 debug
layer disabled: the InfoQueue mismatch is now addressed in code, while leaving
the validation layer enabled added substantial overhead and was present in
the final crash stack. GPU-based validation remains off. CI passing will
validate compilation and tests, not guarantee that the runtime no longer
crashes.

## Why this work is needed

## Windows D3D12 runtime prerequisite (fixed in PR #7)

The test log showed that title loading and import binding completed, but host
D3D12 device creation failed with `0x887E0003` (invalid Agility SDK runtime
configuration). The subsequent XDXR root-signature error was downstream of
that host-device failure; the mesh fallback had not started executing.

The executable requests Agility SDK 1.620.2 through its exported
`D3D12SDKVersion` and `D3D12SDKPath` symbols. The matching
`D3D12Core.dll` must be beside `victory.exe` under its app-local
`D3D12\\` directory. PR #7 now:

- stages the pinned Microsoft NuGet runtime through `cargo xtask agility`,
  verifies the package SHA-512, and writes a version marker beside the DLLs;
- makes `scripts/play.ps1` check that the matching runtime exists and stage it
  automatically before starting the game;
- adds CI that performs the actual package download/extraction, builds the
  release executable, checks both exports with `dumpbin`, and parses the
  PowerShell launcher.

This CI passed on Windows Server 2025. It verifies packaging and executable
configuration, not compatibility on the user's Windows 10 IoT LTSC host; that
still needs a local launch test.


On mesh-capable adapters, the native path uses a D3D12 mesh pipeline
and `ID3D12GraphicsCommandList6::DispatchMesh`. On adapters without native
mesh shaders, PR #7 instead selects the experimental compute/vertex fallback
described below. The fallback is a real conversion, not a capability override.

The NVIDIA GTX 1050 Ti is a Pascal GPU without D3D12 mesh-shader support. It
cannot execute the native mesh path; it must use the translated compute
producer and generated conventional vertex draw.

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

The normal translate() and emit() paths remain native and unchanged by
default. The fallback pipeline builder now consumes this API, allocates bounded
scratch outputs, binds the compute and generated-vertex passes, inserts the
required transitions, and issues conventional draws. This is still an
experimental implementation, not a verified playable GTX 1050 Ti build.

## Milestone 3: direct-dispatch GPU integration (implemented in PR #7)

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

The implementation explicitly refuses non-triangle mesh topologies. The
fallback is covered by synthetic layout/argument tests and Windows CI, but
physical-GPU and output-equivalence validation are still outstanding.

## Milestone 4: indirect dispatch conversion (implemented in PR #7)

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
- The current Windows CI run passes Clippy, the diagnostics build, the full
  workspace test suite, Agility SDK download/hash/extraction, a Release build,
  executable-export verification, and PowerShell launcher parsing. Physical-GPU
  validation, output-equivalence, and target-game rendering remain outstanding.

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
4. **Indirect dispatch:** implemented for bounded one-dimensional records,
   including the console `DispatchMeshX` callback; extend coverage for title
   variants and verify ordering on hardware.
5. **Wave behavior:** expand tests for the existing wave64-to-wave32 lowering,
   and confirm that unsupported 64-lane patterns are refused.
6. **Validation:** add synthetic buffer/image equivalence tests, run DXIL
   validation and workspace Clippy/tests, then test on a non-mesh GPU. Report the
   exact adapter, driver, Agility runtime, VRAM/RAM, settings, frame captures
   and failure logs. Do not call the game playable based on pipeline creation
   or non-black frames alone.

## Current limitations

- Direct and indirect GPU fallback paths are implemented in PR #7; Windows CI
  passes, but physical-GPU validation remains outstanding.
- Indirect fallback is limited to one-dimensional records, 65,535 records per
  batch, and a bounded output region; excess work is zeroed.
- Console `DispatchMeshX` indirect calls now route into the existing mesh
  `ExecuteIndirect` path rather than being discarded.
- No synthetic output-content/image-equivalence or physical GPU test has
  passed on this repository's snapshot.
- Some mesh shaders still require wave64 and are refused on 32-lane hosts.
- The census is a diagnostic milestone, not compatibility support.
- The fallback's root signatures use directly indexed descriptor heaps. This
  requires Shader Model 6.6 and Resource Binding Tier 3; startup now queries
  both and refuses the fallback with a specific log message when either is
  unavailable. The GTX 1050 Ti's actual driver/runtime report must be checked.
- The GTX 1050 Ti has 4 GB of VRAM; even a correct fallback may be limited by
  temporary output buffers, the game's broader memory footprint and CPU/GPU
  translation costs.
- Upstream synthetic-test results do not guarantee the target title will
  render correctly or perform acceptably on this machine.

## Runtime fixes from the follow-up review

The fallback now uses two root signatures rather than sharing one between
compute and graphics:

- The graphics signature exposes mesh-stage bindings to the generated vertex
  shader only. Pixel-stage bindings retain pixel visibility.
- The compute signature contains only the mesh-stage parameters and static
  samplers, with ALL visibility, followed by the three output UAVs.
- Indirect compute and draw command signatures are created against their
  respective root signatures.

This avoids the D3D12 root-signature visibility collision that can otherwise
refuse fallback pipelines when both mesh and pixel shaders use overlapping
registers. The generated vertex shader's output SRVs stay in the graphics
signature; compute UAVs stay in the compute signature.

The per-list data scratch pool now rewinds its allocation cursor before each
sequential mesh fallback output allocation. This is safe only because each
direct or indirect fallback path records the output read and transitions the
buffer back to UAV before the next allocation can overwrite it. The pool keeps
a bounded budget and uses 4 MiB growth chunks (larger individual allocations
are still sized to fit).

These changes address two known causes of real-device failure: invalid root
signatures and scratch-buffer growth across thousands of mesh draws. They do
not fix unrelated game memory use, untranslated shaders, wave64 emulation, or
guarantee playable performance on the GTX 1050 Ti.

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

The console `DispatchMeshX` callback now routes non-zero indirect argument
addresses through the existing mesh `ExecuteIndirect` path (kind 10, 12-byte
group-count records). That path clamps the title's argument records and
selects the compute/vertex fallback when the active host pipeline requires it.
Previously these calls were logged and dropped, which could prevent world
geometry work from being submitted at all.

These are correctness fixes, not proof of output equivalence. The exact
group-base duplication described above was removed and covered by a
group-index unit test. The GTX 1050 Ti remains unverified on physical hardware.


## Latest GTX 1050 Ti run (2026-11-25)

The uploaded `ztrx(7).log` confirms that the Agility SDK startup blocker is
past the first gate: the process maps its game images and logs
`host D3D12 device ready`. The Windows system copy of `D3D12Core.dll` was
also present. This does not mean the mesh fallback has succeeded.

The log confirms that D3D12 device creation itself succeeded, but it
still exposes several renderer problems:

1. The fallback compute PSO was refused 870 times with `0x80070057`
   (`E_INVALIDARG`). Seven additional ordinary host pipeline creations were
   refused. Because this was a Release run without the D3D12 debug layer, it
   does not identify which DXIL or PSO input caused those errors.
2. The renderer logged 96,197 failed unordered-access buffer allocations:
   96,194 requested 32 MiB and three requested 1 MiB. Of these, 95,828
   returned `0x8007000E` (out of memory) and 369 returned
   `0x887A0005` (device suspended). The first repeated failures start at
   approximately 23:35:30 UTC. This is a scratch-buffer allocation problem;
   the log alone does not prove that all GPU memory was exhausted by this
   fallback.
3. The title was still told that its render mode was 3840x2160. Fitting the
   window to the desktop does not reduce the render mode passed to the title.
4. The device was eventually removed after submission of 25 command lists.
   No DRED breadcrumbs identify the offending command.

The reference repository has the same deliberately unimplemented console
pipeline method at slot `0x218`; its `device.rs` comments say the description
format is unknown and it refuses the call rather than returning a fake PSO.
That warning is therefore not, by itself, evidence of a new regression in
this branch.

### Corrections made after this log

Source review found a concrete mesh-index bug that matches the scattered
triangle/particle-like rendering symptom. For non-expanded mesh draws, the
lowerer added `groupId.x * max_vertices` once in `emit_indices` and again
when `mesh_out_store_at` wrote the index buffer. This double-rebased every
group after group zero, pointing indices past their intended vertex records.
The extra addition in `emit_indices` is now removed; the shared store helper
is the only rebasing point for indexed draws. Expanded draws retain local
indices because the generated vertex shader applies the group base from
`SV_VertexID`. A unit test covers both cases.

The 32 MiB allocation-failure path also retried the same failed D3D12
allocation on every mesh output request. The mesh scratch pool now remembers
a refused allocation size until that command-list pool resets, and a unit test
checks that it makes only one attempt per size before reset. This prevents the
retry storm; it does **not** guarantee that a 32 MiB allocation will succeed
on the GTX 1050 Ti.

The reference repository's integer-render-target blend correction was also
ported: graphics PSOs now disable blending only on integer render targets,
preserving the blend settings on other targets. This handles a D3D12-invalid
blend-state combination, but it does not by itself explain the separate
mesh-compute PSO `E_INVALIDARG` errors.

Finally, `scripts/play.ps1` now defaults to a reported `1280x720` mode when
`VICTORY_MESH_FALLBACK=1` and no explicit `VICTORY_DISPLAY_MODE` is set.
An explicit mode is preserved. The old log's 3840x2160 mode predates this
launcher change.

### Focused diagnostic run

Set the title's reported output mode before launching, so the log should say
`display mode reported to the title width=1280 height=720`:

~~~powershell
$env:VICTORY_MESH_FALLBACK = "1"
Remove-Item Env:VICTORY_DISPLAY_MODE -ErrorAction SilentlyContinue
$env:VICTORY_D3D12_DEBUG = "1"

cargo build --profile diag -p victory --features diagnostics

.\\scripts\\play.ps1 -Exe ".\\target\\diag\\victory.exe" -Game "C:\\PATH\\TO\\YOUR\\GAME.exe"

notepad "$env:TEMP\\ztrx.log"
~~~

The diagnostics build enables the D3D12 debug layer; when a pipeline is
refused, the runtime drains its InfoQueue messages into the log. Keep the
environment variables in the same PowerShell window used to build and launch.
The launcher stages the pinned Agility runtime for the selected executable
profile. Replace the game path with the full path to the title executable.
The first diagnostic run may be substantially slower; stop it with Ctrl+C if
it becomes unresponsive. Do not enable GPU-based validation on the first pass.

A useful result must include the first D3D12 validation messages near the first
`mesh fallback compute pipeline refused` or `host pipeline refused` line,
the reported display mode, and the last frame/memory lines. If the run still
reports `3840x2160`, the display-mode override did not reach the process and
the resolution comparison is inconclusive.

## Latest GTX 1050 Ti run (2026-11-26; `ztrx(8).log`)

The new log confirms that the launcher **did** report `1280x720` to the title.
The separate lifecycle line saying the 3840x2160 window was fitted to the desktop
describes the screen/window fit; it does not override the title's 1280x720 render
mode. The resolution override is therefore no longer the blocker.

The renderer still has substantial correctness failures:

- The log contains 181 mesh shaders translated to compute, but **238 mesh
  fallback compute-pipeline creation refusals** with `0x80070057`
  (`E_INVALIDARG`). It also reports 14 ordinary host-pipeline refusals.
  These PSO failures are a direct reason mesh draws are unavailable. This
  Release log did not enable the D3D12 debug layer, so it does not contain the
  InfoQueue explanation for the rejected compute PSOs.
- The title requested a view heap with 1,245,184 entries. The NVIDIA host
  could not create a shader-visible view heap above one million descriptors,
  so the fallback heap reserves 262,144 dynamic per-draw slots and mirrors
  title descriptors in the remaining 737,855 slots. The log did **not** emit
  the separate `title slot past the host heap` warning, so this cap is a
  compatibility limit to watch, not proof that this run actually read an
  out-of-range title descriptor. Raising the heap limit blindly is unsafe on
  this NVIDIA path.
- The log reports 310 `0x8007000E` out-of-memory failures, 147 missing-host-
  texture events, 13 upload-buffer allocation failures, and eight
  `0x887A0005` device-suspended errors. It also records an unordered placed
  texture view of dimension 3 that the current host descriptor mirror cannot
  represent; writes through that view are lost.
- At a 0.5 fps frame, frame time reached 2,038 ms. Another slow frame reached
  about 2,005 ms, with 867.9 ms of GPU work and 319.5 ms of copy-back. Other
  slow frames show heavy CPU preparation and many draws dropped because their
  host pipelines were unavailable. The device was eventually suspended during
  texture/resource work; no DRED breadcrumbs identify the exact GPU command.

These findings match the user's report of only a few visible models and a black
3D cutscene better than a simple resolution problem: many mesh compute PSOs are
rejected, descriptor indices past the cap are null, texture/resource allocation
is failing, and one class of placed 3D UAV view is not mirrored. The log does
not prove which single failure causes each missing model or the black cutscene.
The reference `VIctory-7900XTX` repository also leaves placed unordered texture
views unmirrored, so copying that code does not solve this gap.

### CPU/GPU scheduling update

The fallback launcher profile now:
- keeps the title at 1280x720 unless the user explicitly chooses another mode;
- caps card-side title-buffer shadows at 256 MiB by default to leave more room
  for textures and render resources;
- batches repeated GPU hash work with `VICTORY_CARD_REHASH_RUNS=1`;
- enables parallel CPU snapshots, merges, and host-copy refreshes with
  `VICTORY_WIDE_BACK=1`, `VICTORY_WIDE_MERGE=1`, and
  `VICTORY_WIDE_REFRESH=1`, while preserving explicit user overrides.

On the i7-6700, Windows normally reports eight logical processors. The copy
pool is sized for three persistent workers plus the submitting thread
(approximately four physical cores). Pipeline compilation already uses up to
half the reported logical processors; the runtime now logs the number actually
started. These settings let independent work use the available cores; they do
not force the GPU to 100% utilization or guarantee a frame rate. Asynchronous
queue relay remains opt-in until rendering and ordering are proven correct.

### Next required hardware diagnostic

The next run must use the diagnostics executable. The D3D12 debug layer is
required to explain the 238 mesh fallback PSO refusals. Run the focused
diagnostic command above and provide the first InfoQueue errors around the
first `mesh fallback compute pipeline refused` message, plus the frame and
memory lines near device suspension. Do not enable GPU-based validation on
the first pass. This test is to identify the rejected PSO/resource contract;
it is not yet a performance acceptance test.

## Reference comparison: avatarmanz/VIctory-7900XTX

The [VIctory-7900XTX repository](https://github.com/avatarmanz/VIctory-7900XTX)
is useful as a reference for texture/resource lifetime, descriptor tracking,
asynchronous upload ordering, indirect batching, and shader translation
corrections. Its tested machine is a very different target: a Radeon RX 7900
XTX with native D3D12 mesh-shader support, paired with a high-end CPU and
substantially more memory. Its native mesh pipeline cannot be copied wholesale
to the GTX 1050 Ti.

The local branch also now uses the reference's integer-target blend-state
rule in its two conventional graphics-PSO builders. That was a small
self-contained correctness fix with unit coverage, unlike the reference's
larger upload/descriptor lifecycle work.

One particularly relevant reference feature is
`VICTORY_SHOW_AFTER=1`: it delays making a new texture descriptor visible
until the submission carrying that texture's upload has completed, using a
neutral texture in the meantime. The reference integrates this with descriptor
slot flips, upload jobs, and draw-time descriptor mirroring across its much
larger `descriptor/host.rs` and `draw.rs` implementations. This repository
does not currently implement that feature; merely setting the environment
variable here has no effect. It remains a candidate for a later,
carefully ported texture-correctness milestone, after the outstanding
mesh-compute PSO validation and scratch-allocation behavior have been measured
again.

Do not port the reference's entire descriptor or indirect implementation as
a single change. Its code is from a different source snapshot and assumes
native mesh shaders on RDNA3. Compare individual resource-lifetime invariants
and synthetic tests against this branch before adapting anything.
