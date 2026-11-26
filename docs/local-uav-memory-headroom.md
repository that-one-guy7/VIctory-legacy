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
