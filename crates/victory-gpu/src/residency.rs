//! Host and GPU memory-pressure handling for texture staging and idle host-side copies.
//!
//! This is not a global GPU-memory quota. It leaves required render targets,
//! mesh scratch, and indirect buffers on their existing allocation paths.
//! It pauses new texture staging after an actual D3D12 OOM, when OS memory is
//! critically low, or when DXGI reports that local/shared GPU memory is near
//! its budget. Under pressure it shortens the lifetime of unreferenced idle
//! copies; resources still held by a list or descriptor are never evicted.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
const OOM_RETRY_MS: u64 = 500;
const GPU_BUDGET_PRESSURE_PERCENT: u64 = 90;
const GPU_BUDGET_RECOVERY_PERCENT: u64 = 80;
const GPU_PRESSURE_IDLE_MS: u64 = 1_000;
const GPU_PRESSURE_IDLE_FRAMES: u64 = 1;
const CRITICAL_PHYSICAL_FREE: u64 = 512 * MIB;
const CRITICAL_COMMIT_FREE: u64 = 512 * MIB;

static PRESSURED: AtomicBool = AtomicBool::new(false);
static GPU_PRESSURED: AtomicBool = AtomicBool::new(false);
static NEXT_SAMPLE_AT: AtomicU64 = AtomicU64::new(0);
static PHYSICAL_FREE: AtomicU64 = AtomicU64::new(u64::MAX);
static COMMIT_FREE: AtomicU64 = AtomicU64::new(u64::MAX);
static RETRY_TEXTURES_AT: AtomicU64 = AtomicU64::new(0);
static LAST_DEFER_WARNING_AT: AtomicU64 = AtomicU64::new(0);

fn memory_status() -> Option<(u64, u64)> {
    // SAFETY: the API initializes a correctly sized MEMORYSTATUSEX structure.
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    (unsafe { GlobalMemoryStatusEx(&mut status) } != 0)
        .then_some((status.ullAvailPhys, status.ullAvailPageFile))
}

/// Samples at most every 32 ms, even when one scene-load pass takes seconds.
fn sample_memory(now: u64) -> Option<(u64, u64)> {
    let next = NEXT_SAMPLE_AT.load(Ordering::Relaxed);
    if now >= next
        && NEXT_SAMPLE_AT
            .compare_exchange(
                next,
                now.saturating_add(32),
                Ordering::Relaxed,
                Ordering::Relaxed,
            )
            .is_ok()
        && let Some((physical, commit)) = memory_status()
    {
        PHYSICAL_FREE.store(physical, Ordering::Relaxed);
        COMMIT_FREE.store(commit, Ordering::Relaxed);
    }

    let physical = PHYSICAL_FREE.load(Ordering::Relaxed);
    let commit = COMMIT_FREE.load(Ordering::Relaxed);
    (physical != u64::MAX && commit != u64::MAX).then_some((physical, commit))
}

fn next_pressure(was: bool, physical: u64, commit: u64) -> bool {
    if was {
        // Hysteresis prevents repeated enter/leave transitions around a limit.
        physical < 5 * GIB || commit < 6 * GIB
    } else {
        physical <= 3 * GIB || commit <= 4 * GIB
    }
}

fn critical_memory(physical: u64, commit: u64) -> bool {
    physical < CRITICAL_PHYSICAL_FREE || commit < CRITICAL_COMMIT_FREE
}

/// Whether a DXGI memory segment has reached the configured budget threshold.
/// A missing/zero budget is treated as unknown rather than as pressure.
fn gpu_budget_pressure(budget: u64, used: u64, percent: u64) -> bool {
    budget != 0 && used >= budget.saturating_mul(percent.min(100)) / 100
}

/// Called by the once-per-second host maintenance sweep.
pub(crate) fn refresh() {
    let now = crate::host::now_ms();
    if let Some((physical, commit)) = sample_memory(now) {
        let was = PRESSURED.load(Ordering::Relaxed);
        let pressure = next_pressure(was, physical, commit);
        if pressure != was {
            PRESSURED.store(pressure, Ordering::Relaxed);
            if pressure {
                tracing::warn!(
                    physical_free_mib = physical / MIB,
                    available_commit_mib = commit / MIB,
                    "host memory pressure: shortening idle-copy retention; required rendering allocations remain enabled"
                );
            } else {
                tracing::info!(
                    physical_free_mib = physical / MIB,
                    available_commit_mib = commit / MIB,
                    "host memory pressure cleared"
                );
            }
        }
    }

    // DXGI's per-segment budgets can be exhausted while Windows still has
    // several GiB of physical RAM and commit headroom. Sample both local VRAM
    // and the non-local segment so upload allocations are deferred before the
    // driver starts failing them.
    if let Some([local_budget, local_used, shared_budget, shared_used]) =
        crate::host::host().and_then(|host| host.video_memory())
    {
        // Use hysteresis so small DXGI budget fluctuations do not make
        // staging repeatedly stop and restart around one threshold.
        let threshold = if GPU_PRESSURED.load(Ordering::Relaxed) {
            GPU_BUDGET_RECOVERY_PERCENT
        } else {
            GPU_BUDGET_PRESSURE_PERCENT
        };
        let gpu_pressure = gpu_budget_pressure(local_budget, local_used, threshold)
            || gpu_budget_pressure(shared_budget, shared_used, threshold);
        let was_gpu_pressure = GPU_PRESSURED.swap(gpu_pressure, Ordering::Relaxed);
        if gpu_pressure != was_gpu_pressure {
            if gpu_pressure {
                tracing::warn!(
                    local_budget_mib = local_budget / MIB,
                    local_used_mib = local_used / MIB,
                    shared_budget_mib = shared_budget / MIB,
                    shared_used_mib = shared_used / MIB,
                    threshold_percent = GPU_BUDGET_PRESSURE_PERCENT,
                    "GPU memory budget pressure: deferring new texture staging and shortening idle-copy retention"
                );
            } else {
                tracing::info!("GPU memory budget pressure cleared");
            }
        }
    }
}

/// Records an actual D3D12 allocation failure. Only E_OUTOFMEMORY triggers
/// the cooldown; unrelated device/driver errors keep their existing behavior.
pub(crate) fn allocation_error(error: &windows::core::Error) {
    if error.code() != windows::Win32::Foundation::E_OUTOFMEMORY {
        return;
    }
    let now = crate::host::now_ms();
    let retry_at = now.saturating_add(OOM_RETRY_MS);
    let previous = RETRY_TEXTURES_AT.fetch_max(retry_at, Ordering::Relaxed);
    if previous <= now {
        tracing::warn!(
            retry_after_ms = OOM_RETRY_MS,
            hresult = %error.code(),
            "D3D12 returned E_OUTOFMEMORY; pausing new texture staging briefly"
        );
    }
}

fn warn_deferred(now: u64, bytes: u64, reason: &'static str, physical: u64, commit: u64) {
    let previous = LAST_DEFER_WARNING_AT.load(Ordering::Relaxed);
    if now < previous.saturating_add(2_000)
        || LAST_DEFER_WARNING_AT
            .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
    {
        return;
    }
    tracing::warn!(
        request_mib = bytes.div_ceil(MIB),
        reason,
        physical_free_mib = (physical != u64::MAX).then_some(physical / MIB),
        available_commit_mib = (commit != u64::MAX).then_some(commit / MIB),
        "deferring a new texture staging allocation; it will be retried on a later use"
    );
}

/// Whether a newly sampled texture may allocate its host upload staging buffer.
/// Existing textures and in-flight lists are untouched and can still render.
pub(crate) fn allow_texture_staging(bytes: u64) -> bool {
    let now = crate::host::now_ms();
    if GPU_PRESSURED.load(Ordering::Relaxed) {
        warn_deferred(
            now,
            bytes,
            "DXGI local/shared GPU memory budget pressure",
            PHYSICAL_FREE.load(Ordering::Relaxed),
            COMMIT_FREE.load(Ordering::Relaxed),
        );
        return false;
    }
    let retry_at = RETRY_TEXTURES_AT.load(Ordering::Relaxed);
    if now < retry_at {
        warn_deferred(
            now,
            bytes,
            "recent D3D12 out-of-memory result",
            PHYSICAL_FREE.load(Ordering::Relaxed),
            COMMIT_FREE.load(Ordering::Relaxed),
        );
        return false;
    }

    let Some((physical, commit)) = sample_memory(now) else {
        // If Windows cannot provide a memory sample, preserve the old behavior.
        return true;
    };
    if critical_memory(physical, commit) {
        warn_deferred(now, bytes, "critically low Windows memory headroom", physical, commit);
        return false;
    }
    true
}

/// Shorten idle retention only while sustained host memory pressure is active.
/// The existing reference-count and command-list ownership checks remain intact.
fn pressure_idle_timeout_ms(pressured: bool, normal: u64) -> u64 {
    if pressured {
        normal.min(2_500)
    } else {
        normal
    }
}

fn pressure_idle_timeout_frames(pressured: bool, normal: u64) -> u64 {
    if pressured {
        normal.min(2)
    } else {
        normal
    }
}

fn gpu_pressure_idle_timeout_ms(pressured: bool, normal: u64) -> u64 {
    if pressured {
        normal.min(GPU_PRESSURE_IDLE_MS)
    } else {
        normal
    }
}

fn gpu_pressure_idle_timeout_frames(pressured: bool, normal: u64) -> u64 {
    if pressured {
        normal.min(GPU_PRESSURE_IDLE_FRAMES)
    } else {
        normal
    }
}

pub(crate) fn idle_timeout_ms(normal: u64) -> u64 {
    let gpu = GPU_PRESSURED.load(Ordering::Relaxed);
    if gpu {
        gpu_pressure_idle_timeout_ms(gpu, normal)
    } else {
        pressure_idle_timeout_ms(PRESSURED.load(Ordering::Relaxed), normal)
    }
}

pub(crate) fn idle_timeout_frames(normal: u64) -> u64 {
    let gpu = GPU_PRESSURED.load(Ordering::Relaxed);
    if gpu {
        gpu_pressure_idle_timeout_frames(gpu, normal)
    } else {
        pressure_idle_timeout_frames(PRESSURED.load(Ordering::Relaxed), normal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_budget_pressure_uses_a_strict_budget_threshold() {
        assert!(!gpu_budget_pressure(4_000, 3_599, 90));
        assert!(gpu_budget_pressure(4_000, 3_600, GPU_BUDGET_PRESSURE_PERCENT));
        assert!(gpu_budget_pressure(4_000, 4_100, GPU_BUDGET_PRESSURE_PERCENT));
        assert!(!gpu_budget_pressure(4_000, 3_199, GPU_BUDGET_RECOVERY_PERCENT));
        assert!(gpu_budget_pressure(4_000, 3_200, GPU_BUDGET_RECOVERY_PERCENT));
        assert!(!gpu_budget_pressure(0, u64::MAX, 90));
        assert!(gpu_budget_pressure(u64::MAX, u64::MAX, 100));
    }

    #[test]
    fn pressure_enters_and_recovers_with_hysteresis() {
        assert!(next_pressure(false, 3 * GIB, 8 * GIB));
        assert!(next_pressure(false, 8 * GIB, 4 * GIB));
        assert!(next_pressure(true, 4 * GIB, 8 * GIB));
        assert!(next_pressure(true, 8 * GIB, 5 * GIB));
        assert!(!next_pressure(true, 5 * GIB, 6 * GIB));
        assert!(!next_pressure(false, 8 * GIB, 8 * GIB));
    }

    #[test]
    fn only_critical_host_memory_blocks_new_texture_staging() {
        assert!(critical_memory(511 * MIB, 8 * GIB));
        assert!(critical_memory(8 * GIB, 511 * MIB));
        assert!(!critical_memory(512 * MIB, 8 * GIB));
        assert!(!critical_memory(8 * GIB, 512 * MIB));
        assert!(!critical_memory(GIB, 8 * GIB));
        assert!(critical_memory(256 * MIB, 8 * GIB));
        assert!(critical_memory(8 * GIB, 256 * MIB));
    }

    #[test]
    fn pressure_shortens_only_idle_retention() {
        assert_eq!(10_000, pressure_idle_timeout_ms(false, 10_000));
        assert_eq!(2_500, pressure_idle_timeout_ms(true, 10_000));
        assert_eq!(2, pressure_idle_timeout_frames(true, 4));
        assert_eq!(4, pressure_idle_timeout_frames(false, 4));
        assert_eq!(1_000, gpu_pressure_idle_timeout_ms(true, 10_000));
        assert_eq!(10_000, gpu_pressure_idle_timeout_ms(false, 10_000));
        assert_eq!(1, gpu_pressure_idle_timeout_frames(true, 4));
        assert_eq!(4, gpu_pressure_idle_timeout_frames(false, 4));
    }
}
