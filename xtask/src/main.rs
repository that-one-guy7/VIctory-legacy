//! Repository maintenance. Run with `cargo xtask <command>`.

mod agility;
mod policy;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "xtask")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Enforce the asset policy: the repository holds only small text files
    /// we wrote. Checks tracked and untracked-but-not-ignored files.
    Policy {
        /// Check only what is staged for commit (used by the pre-commit hook).
        #[arg(long)]
        staged: bool,
    },
    /// Point git at .githooks/ so every commit runs the policy check.
    InstallHooks,
    /// Fetch the D3D12 Agility SDK runtime (Microsoft's NuGet package,
    /// pinned and hash-checked) into `<target>/<profile>/D3D12/` beside
    /// victory.exe. Needed on Windows 10, whose in-box D3D12 stops at shader
    /// model 6.5.
    Agility {
        /// Cargo's target folder. Default: `CARGO_TARGET_DIR`, else `target`.
        #[arg(long)]
        target_dir: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let root = workspace_root();
    match Cli::parse().command {
        Cmd::Policy { staged } => policy::run(&root, staged),
        Cmd::InstallHooks => {
            let status = Command::new("git")
                .args(["config", "core.hooksPath", ".githooks"])
                .current_dir(&root)
                .status()
                .context("running git")?;
            anyhow::ensure!(
                status.success(),
                "git config failed (is this a git repository?)"
            );
            println!("hooks installed: commits now run `cargo xtask policy --staged`");
            Ok(())
        }
        Cmd::Agility { target_dir } => {
            let target = target_dir
                .or_else(|| std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from))
                .unwrap_or_else(|| root.join("target"));
            agility::run(&target)
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives in the workspace")
        .into()
}
