# GPU-local UAV allocation headroom

The host creates default-heap D3D12 UAV buffers for mesh-fallback scratch,
indirect arguments, title/card-buffer shadows, depth-twin staging, and texture
alias-copy staging. These allocations are separate from indexed texture copies
managed by `VICTORY_TEXTURE_BUDGET_MB` and
`VICTORY_LOCAL_TEXTURE_BUDGET_MB`.

The 85% DXGI local-memory headroom check applies only to **optional**
optimizations: card-buffer shadows and hash staging, depth twins, and texture
alias-copy staging. If the requested optional allocation would cross that
threshold, the optimization is skipped and its normal non-optimized path is
used. Required mesh scratch and indirect-argument allocations retain their
previous allocation behavior; refusing those buffers could prevent a frame
from being submitted or break the mesh fallback. If DXGI cannot provide a
budget, optional allocations retain the old behavior.

This is a conservative admission check, **not a global VRAM quota**. DXGI's
usage is a snapshot and concurrent allocations can race; textures, render
targets, required scratch/indirect buffers, other committed resources, and
operating-system allocations are not all routed through this optional guard.
Skipping an optional optimization can change performance or image behavior,
so a physical run is still needed to confirm the result on a particular
adapter. CI tests the threshold logic and fallback decisions, not real GPU
stability or frame rate.

## Mesh-fallback scratch across all command lists

The per-pool 64 MiB reuse limit is not an aggregate limit: every host command
list owns two mesh-scratch pools, and many lists can remain alive together.
A workload with hundreds of lists could therefore retain several gigabytes
of default-heap UAV scratch even though no individual pool exceeded 64 MiB.

New allocations now share a process-wide local-scratch allowance:
- 15% of the adapter's DXGI local-memory budget, capped at 512 MiB by default;
- no new local scratch when current local-memory usage reaches 80% of budget;
- when either limit is reached, new mesh scratch uses a CPU-visible,
  shared-memory UAV buffer instead of another default-heap allocation.

`VICTORY_MESH_LOCAL_BUDGET_MB` can impose a lower allowance. It cannot raise
the automatic cap. The byte counter is released when the owning command-list
pool is dropped.

The shared-memory path is valid for this scratch because these buffers are
created with unordered-access support; the GPU can write them, but reads and
writes can be slower than local VRAM. The fallback is attempted only for new
mesh scratch allocations. It does not migrate old chunks, cap all D3D12
allocations, or change texture/render-target allocation. A failed shared
allocation can still prevent that mesh operation, and device stability must
be confirmed on the target GPU.

The launcher/copy statistics also report device-local UAV buffers as a
cumulative "made so far" count; that number is not the live size of mesh
scratch. Diagnostics now print live mesh-scratch chunk counts and bytes for
both local and shared-memory chunks, so the next hardware log can verify how
much this pool actually retains.


## Stricter low-VRAM fallback profile

When `VICTORY_MESH_FALLBACK=1`, the launcher supplies conservative defaults
unless a process-level value is already set:

- `VICTORY_MESH_LOCAL_BUDGET_MB=128`: limits retained local mesh scratch to
  128 MiB; new scratch beyond the limit uses CPU-visible shared memory.
- `VICTORY_MESH_SHARED_SPARE_MB=32`: after a list's submission finishes,
  its shared-memory mesh scratch chunks are returned to one process-wide
  reuse cache. The cache evicts old chunks over 32 MiB, releasing their
  system RAM. This avoids every long-lived host list retaining a private
  collection of shared chunks. Scratch concurrently needed by one submission
  is not capped by this idle-cache limit.
- `VICTORY_LOCAL_UAV_BUDGET_PERCENT=70`: optional UAV optimizations stop
  requesting local allocations at 70% of the DXGI budget (the general
  default remains 85%). Required mesh scratch and indirect-argument buffers
  are not denied by this optional-allocation gate.
- `VICTORY_RENDER_SCALE=1.0`: keep host render/depth targets at native
  dimensions. The latest GTX 1050 Ti log showed a `CopyResource` being skipped
  because one target was at 0.5 scale while the other remained at 1.0. Keep
  target scaling opt-in until the wrapper can safely handle those mixed-scale
  copy dependencies.
- `VICTORY_TEXTURE_MIP_DROP=3`: eligible large sampled textures omit their
  top three mip levels on the host copy. This is the most aggressive supported
  preset and can noticeably reduce texture detail; do not increase it further.

The render-scale and mip-drop paths each have eligibility limits described
in their respective documentation. They do not rewrite the title's resource
descriptions, and they do not reduce every texture or render target. Texture
mip dropping and render scaling reduce visual detail; the shared-memory mesh
scratch fallback can reduce performance. Explicit process environment values
still take precedence over these launcher defaults.

For a clean test after earlier manual experiments, remove stale overrides for
these profile settings before starting the launcher. The diagnostic header
records the effective values so a log can confirm which profile actually ran.
