# Diagnostic log contents

The normal PowerShell launcher writes a diagnostic report to `%TEMP%\\ztrx.log`
before starting the game, then appends the process output and final exit code.

The report includes:

- The wrapper executable, game executable, Cargo profile, and expected/installed
  D3D12 Agility SDK version
- PowerShell and Windows version information
- Every effective `VICTORY_*` process environment variable, sorted by name
- Best-effort physical-memory totals/free memory, CPU model/logical processor
  count, and display-adapter names/driver versions/reported adapter memory
- The launcher's final exit code and completion timestamp

Only variables prefixed with `VICTORY_*` are recorded. The launcher does not
dump the full process environment, which could contain unrelated credentials
or private settings. Hardware queries use Windows CIM and are best-effort; a
CIM/WMI failure is written as an unavailable field and does not block launch.

## Before uploading a log

The report contains full paths to the wrapper and game executable and lists
the values of all `VICTORY_*` variables. Review the file before sharing it
publicly and remove any path or custom setting you do not want to disclose.

## Recommended low-VRAM test record

For a useful before/after comparison, keep the game scene, save, driver,
background applications, and other settings the same. Change only one
experimental variable per run. For render scaling, test `1.0`, then `0.75`,
then `0.5`; note whether the scene is visually correct, whether it reaches
the same point, and whether the process exits or crashes. A successful CI run
does not establish real GPU correctness or performance.
