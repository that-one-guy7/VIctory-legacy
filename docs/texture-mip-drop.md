# Opt-in sampled-texture mip reduction

This experimental switch omits the highest-resolution mip levels only from host copies of eligible large, power-of-two, single-sample 2D sampled textures. Render targets, depth textures, unordered-access textures, multisampled textures, non-power-of-two textures, unsupported swizzle layouts, and textures smaller than the size floor are not reduced.

The default is off. Set `VICTORY_TEXTURE_MIP_DROP=1` to omit one top mip. Values 2 and 3 are more aggressive and should only be tried after a level-1 run. The size floor defaults to 1024 pixels in both dimensions and can be changed with `VICTORY_TEXTURE_MIP_DROP_MIN_SIZE`.

This is a memory-saving compatibility experiment, not an in-game graphics menu. Dropped top mips are unavailable, so close-up textures may look softer. Some texture copies can only copy mip levels that exist in both host resources.

## Windows test

From the repository root:

```powershell
$env:VICTORY_TEXTURE_MIP_DROP = "1"
$env:VICTORY_TEXTURE_MIP_DROP_MIN_SIZE = "1024"
cargo build --profile diag -p victory --features diagnostics

$r="$env:USERPROFILE\Downloads\VIctory-legacy-work-mesh-fallback-index-correctness-rebased (3)\VIctory-legacy-work-mesh-fallback-index-correctness-rebased"
& "$r\scripts\play.ps1" -Exe "$r\target\diag\victory.exe" -Game "C:\GTA 6 (XBOX)\GTA6.Xbox.v1.0.2.1-CYBERLEEK\game_scarlett_master.exe"
```

The launcher writes `%TEMP%\ztrx.log`. Confirm that the log reports `texture mip dropping enabled` and `reducing sampled texture resolution`. Compare how far the same scene loads and upload the log after a crash. Unset the switches to return to the old behavior:

```powershell
Remove-Item Env:VICTORY_TEXTURE_MIP_DROP -ErrorAction SilentlyContinue
Remove-Item Env:VICTORY_TEXTURE_MIP_DROP_MIN_SIZE -ErrorAction SilentlyContinue
```
