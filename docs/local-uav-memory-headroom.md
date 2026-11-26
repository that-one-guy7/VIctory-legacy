# GPU-local UAV allocation headroom

The host's `uav_buffer` path creates default-heap D3D12 buffers used by
mesh-fallback scratch, title/card-buffer shadows, depth-twin staging and
indirect arguments. These allocations are separate from the indexed texture
copies governed by `VICTORY_TEXTURE_BUDGET_MB` and
`VICTORY_LOCAL_TEXTURE_BUDGET_MB`.

When DXGI reports the adapter's local-memory budget and current usage, the
UAV path now declines a new buffer if the request would cross 85% of that
budget. This leaves headroom for the driver, swap chain, and allocations made
through other paths. The warning records the reported budget, current usage,
and refused request. If DXGI cannot provide a budget, the old allocation
behavior is retained.

This is a conservative admission check, **not a global VRAM quota**. DXGI's
usage is a snapshot and concurrent allocations can race; textures, render
targets, other committed resources, and operating-system allocations are not
all routed through this guard. Refusing a buffer can also reduce rendering
functionality if the title treats that resource as essential. CI tests the
threshold logic, but only a physical run can establish whether the fallback
remains visually correct and more stable on a particular adapter.
