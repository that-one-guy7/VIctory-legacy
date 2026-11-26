# Host-memory pressure guard

The `cool_fox` continuation contains useful memory-management work, but its
4 GB launcher is not a drop-in configuration for this older code line. It turns
on renderer ports, on-demand texture residency, and other switches that do not
exist here. The reusable idea is to stop repeated allocation attempts after a
real out-of-memory result and to release only resources already proven idle.

This change adds a small Windows memory-pressure controller with three jobs:

1. Sample available physical RAM and Windows commit headroom at most every
   32 ms. Enter pressure at 3 GiB physical RAM or 4 GiB commit headroom; leave
   only after both recover to 5 GiB and 6 GiB respectively.
2. After an actual D3D12 `E_OUTOFMEMORY`, pause creation of *new texture staging
   buffers* for 500 ms. A later texture use retries automatically. New staging
   is also deferred when either physical RAM or commit headroom is below
   512 MiB. If the Windows query is unavailable, the old allocation behavior
   is preserved.
3. During sustained pressure, shorten the idle retention window for staging
   buffers, title-memory shadow windows, and eligible title-filled texture
   copies. Existing Arc/COM ownership checks remain mandatory: live command
   lists, descriptors, and active texture users keep their resources alive.

The guard deliberately does **not** impose a 4 GiB global allocation cap, block
required render targets, deny required mesh scratch or indirect buffers, migrate
resources already resident on the GPU, or alter render resolution. This avoids
turning an allocation failure into an unconditional missing-resource path.

## What this does and does not address

The GTX 1050 Ti diagnostic reported about 1.1 GiB of free physical RAM,
17.9 GiB of process private bytes, 5.3 GiB of host title-memory copies, and
2.99 GiB of host textures at its final sample. The final
`ID3D12GraphicsCommandList::Close` error was `0x8007000E`, but that result
alone does not distinguish a real allocation failure from an invalid recorded
command or a driver-side command-list failure. This guard prevents a class of
repeat allocation failures and reclaims eligible idle copies; it cannot by
itself prove that the reported `Close` failure is fixed.

The next GTX 1050 Ti run still needs the diagnostics build and its D3D12 debug
layer enabled. Check the new pressure-transition, staging-deferral, sweep, and
`Close` messages together with the DRED command-list name. Shader translation
refusals and the descriptor heap limit remain separate scene-rendering issues.
