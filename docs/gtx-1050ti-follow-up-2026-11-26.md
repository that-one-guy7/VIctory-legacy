# GTX 1050 Ti follow-up diagnostic — 2026-11-26

## What changed since the previous run

The render scale was 1.0 and the log no longer reports a skipped CopyResource
between differently scaled render targets. That removes the previously
identified small-rectangle cause, but it does not make the 3D scene render.
The queue still reported 36 black frames.

The title requested 1,245,184 shader-visible view descriptors, while the
NVIDIA host heap was capped at 1,000,000. The translator clamps indices at or
past the host limit to a null descriptor. This can make title shaders read
the wrong/null resources when they address a descriptor beyond the cap.
The current log does not prove that every black frame is caused by this
specific limit; a compact descriptor-index remapping design is still needed.

The log also has shader translation refusals:

- Compute hash a929109552ce1e84: v_permlanex16_b32 unsupported.
- Compute hash f133e35efed2a48a: lane-mask exchange where not every lane runs.
- Compute hash ceb5eb68cd99cb07: descriptor address/format read as data.
- Pixel hashes 7d0f14ce44746d7e and 6b6db9f1a526daeb: descriptor read only as data.

Their draws are dropped when no host pipeline can be made. The log also reports
a large pending pipeline backlog. These issues require translator/descriptor
work; changing memory limits alone cannot fix them.

## Memory at the final sample

| Metric | Approximate value |
|---|---:|
| DXGI local GPU usage | 3,553 MiB |
| DXGI local budget | 3,403 MiB |
| Shared/non-local GPU memory in use | 6,789 MiB |
| Physical RAM free | 1,125 MiB |
| Process private bytes | 17,896 MiB |
| Process resident memory | 11,553 MiB |
| Windows commit headroom | 44,177 MiB |
| Host copies of title memory | 5,272 MiB |
| Of those, window copies outside buffers | 4,284 MiB |
| Host textures | 2,994 MiB |
| Of those, render/write textures | 2,579 MiB |

The local VRAM reading near 3.4 GiB does not include the 6.8 GiB of shared
GPU-visible system memory. The large commit headroom is page-file/commit
capacity, not physical RAM available to keep GPU-active resources resident.
The mesh-scratch pool was much smaller than the title-memory copies and host
textures in this run.

The process was also CPU-bound: measured frame times ranged roughly from
0.3 to 1 second, and samples showed several GiB of title-memory ranges being
checked/refreshed per frame. That fits the reported CPU saturation.

## Crash

The final error was ID3D12GraphicsCommandList::Close returning 0x8007000E
("not enough memory resources"), followed by exit code -2005270523. This run
did not end with the device-removed code seen in the earlier log. The error
message also says a recorded command was invalid. A separate diagnostics run
with VICTORY_D3D12_DEBUG=1 is needed to identify that command; do not assume
the close error is only ordinary RAM exhaustion.

The diagnostic counter for 278 host lists never reset is not itself proof of
a leak. It counts host-side lists that have never been reset/used; the title
creates list objects that may not be used. Do not forcibly reset them while
the title may still own or use them.

## Next safe experiment: in-place memory probe

The runtime already has an experimental VICTORY_INPLACE path for exposing
eligible title-owned private memory directly to D3D12 rather than maintaining
a separate shared-memory copy. Mode 1 is for windows outside title buffers.
It is currently disabled by default because opening the pages pins them and
can affect the title's memory lifetime.

The diagnostics launcher now sets VICTORY_INPLACE_PROBE=2 for diag builds
using the low-VRAM mesh-fallback profile, unless the user explicitly sets
another value. This probe does not enable VICTORY_INPLACE. It tests the
driver/runtime and samples one part of title memory, then records census
results around 30 and 150 seconds. Use those results before turning on the
in-place window path.

After merging the diagnostic PR, from the repository root:

```powershell
cargo build --profile diag -p victory --features diagnostics --locked

$env:VICTORY_MESH_FALLBACK = "1"
# Leave VICTORY_INPLACE unset for this probe-only run.
Remove-Item Env:VICTORY_INPLACE -ErrorAction SilentlyContinue

$r = (Get-Location).Path
& "$r\scripts\play.ps1" -Exe "$r\target\diag\victory.exe" -Game "C:\GTA 6 (XBOX)\GTA6.Xbox.v1.0.2.1-CYBERLEEK\game_scarlett_master.exe"
```

Let the run proceed normally, with audio enabled, and upload the resulting
%TEMP%\ztrx.log. The 150-second census is useful if the game remains running
that long; do not keep retrying after repeated out-of-memory failures.

If the probe shows that a meaningful amount of window memory is eligible and
the driver's test succeeds, a separate patch can enable VICTORY_INPLACE=1
with a conservative VICTORY_INPLACE_MB cap. Do not combine mode 2 (title
buffers) with VICTORY_LATE_WAIT.
