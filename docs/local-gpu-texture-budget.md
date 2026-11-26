# Local GPU texture budget

VIctory asks DXGI for the adapter's current local-memory budget and usage before
it creates additional host copies for textures selected by descriptor-heap
index. This is a guard on that particular texture-copy path, not a global
D3D12 allocation quota: render targets, UAV buffers, command resources, and
other allocations can still consume memory independently.

## Default behavior

When DXGI reports a local-memory budget, the indexed-texture path stops making
additional copies once this process reaches 85% of that budget. This leaves
headroom for the desktop, driver allocations, render targets, and other GPU
resources. The driver's budget can be lower than the card's advertised VRAM.

## Optional hard ceiling

Set `VICTORY_LOCAL_TEXTURE_BUDGET_MB` to impose a lower absolute ceiling on
local-memory usage for this path. For a 4 GB card, a starting value of
`2800` or `3000` MB is more realistic than allowing the process to consume
all 4096 MB. The effective threshold is the lower of this value and 85% of
the current DXGI budget. It never raises the driver's budget.

The existing `VICTORY_TEXTURE_BUDGET_MB` setting is different: it limits the
alive bytes of the indexed texture copies themselves, not total local VRAM
usage. If set, it retains priority over the new local-usage ceiling.

If DXGI cannot provide a local budget, the existing conservative fallback
remains in effect. The new setting is read once when the texture path first
checks its budget, so set it before launching VIctory.

## Important limitations

This does **not** impose a hard limit on all GPU memory used by VIctory or the
game. It only prevents further indexed-texture copies from being created once
the threshold is reached. Other resource paths need their own budget-aware
allocation and eviction policy. Refusing arbitrary allocations globally would
break resources the game expects to exist and could cause missing rendering or
device removal.

The feature should be validated on the target GPU with DXGI budget/usage logs
and the process's actual peak local-memory use. Passing CI does not establish
that the game remains visually correct or stable under memory pressure.
