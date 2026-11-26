# Experimental host render-resolution scaling

This feature is opt-in and disabled by default. It changes only the D3D12 host copies of eligible render/depth targets; the emulated console resource description, title memory and game-side resource dimensions remain unchanged.

## Settings

```powershell
$env:VICTORY_RENDER_SCALE = "0.5"
$env:VICTORY_RENDER_SCALE_MIN_SIZE = "512"
```

- `VICTORY_RENDER_SCALE=1.0`: native host target dimensions (default).
- `VICTORY_RENDER_SCALE=0.75`: three-quarter width and height.
- `VICTORY_RENDER_SCALE=0.5`: half width and height.
- `VICTORY_RENDER_SCALE=0.25`: quarter width and height.
- `VICTORY_RENDER_SCALE_MIN_SIZE`: minimum original width and height; defaults to 512 pixels.

Only 2D, single-mip, single-sample render-target or depth resources without unordered-access usage are eligible. Sampled-only textures, UAV targets, multisampled targets, mipmapped targets, buffers, and smaller resources retain their original dimensions. The feature logs every host target it reduces.

The wrapper scales viewport coordinates, scissor rectangles and explicit clear rectangles to the reduced host targets. Game-side dimensions and console memory layouts are not rewritten.

## Important limitations

This is an early experiment, not a verified low-resolution mode. GPU copies between scaled and unscaled textures, buffer-to-scaled-texture copies, read-back of scaled targets, and resolves that mix different scales are refused with a warning rather than issuing an invalid D3D12 operation. A game path that relies on one of those operations may show missing or stale output. Multiple render targets are safest when they use the same dimensions and eligibility. The feature does not rewrite arbitrary shader constants or guarantee that every post-processing pass uses the same scale.

Do not treat a successful build or a non-black frame as proof of correctness. Compare the same scene with the option disabled and enabled, inspect the log for `reducing render-target resolution` and any skipped-copy warnings, and verify that depth, shadows, UI and post-processing remain present.

## Low-VRAM profile note

The automatic `VICTORY_MESH_FALLBACK=1` profile leaves this feature disabled
(`VICTORY_RENDER_SCALE=1.0`). A GTX 1050 Ti runtime log showed a
`CopyResource` skipped because the source and destination targets had
different scales. Until mixed-scale copy paths can be converted safely, keep
render-target scaling opt-in and verify the same scene for skipped-copy
warnings whenever testing a value below 1.0.

## Test

From the repository root on Windows:

```powershell
cargo build --profile diag -p victory --features diagnostics --locked

$env:VICTORY_RENDER_SCALE = "0.5"
$env:VICTORY_RENDER_SCALE_MIN_SIZE = "512"
& ".\scripts\play.ps1" -Exe ".\target\diag\victory.exe" -Game "<your game exe>"
```

For the first test, use the same game scene and other environment settings as the baseline run. Upload `%TEMP%\ztrx.log` after the test. To return to native host target sizes:

```powershell
Remove-Item Env:VICTORY_RENDER_SCALE -ErrorAction SilentlyContinue
Remove-Item Env:VICTORY_RENDER_SCALE_MIN_SIZE -ErrorAction SilentlyContinue
```
