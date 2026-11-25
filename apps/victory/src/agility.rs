//! The D3D12 Agility SDK: which D3D12 runtime the host uses.
//!
//! Windows 10's in-box runtime may stop at shader model 6.5 and refuse root
//! signatures that index descriptor heaps directly (flags 0x400, 0x800).
//! These exports opt the process into the pinned Agility SDK runtime. When the
//! in-box runtime is newer than the requested SDK, Windows uses its newer
//! runtime. Otherwise, the matching app-local D3D12Core.dll must exist under
//! the path below; a missing or mismatched DLL makes D3D12CreateDevice fail
//! with D3D12_ERROR_INVALID_REDIST (0x887E0003). Run `cargo xtask agility`
//! to download, hash-check, and stage the pinned runtime.
//!
//! `build.rs` exports both symbols from the executable.
#![allow(non_upper_case_globals)]

/// The runtime's SDK version: the minor number of the
/// `Microsoft.Direct3D.D3D12` package `cargo xtask agility` fetches
/// (1.620.2). Keep the two in step.
#[unsafe(no_mangle)]
pub static D3D12SDKVersion: u32 = 620;

/// A C string pointer D3D12 reads once at device creation.
#[repr(transparent)]
pub struct SdkPath(*const std::ffi::c_char);

// SAFETY: points at a static, immutable C string.
unsafe impl Sync for SdkPath {}

/// Where `D3D12Core.dll` is, relative to the executable.
#[unsafe(no_mangle)]
pub static D3D12SDKPath: SdkPath = SdkPath(c".\\D3D12\\".as_ptr());

/// The folder `D3D12SDKPath` names, beside the running executable.
pub fn runtime_dir() -> Option<std::path::PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join("D3D12"))
}
