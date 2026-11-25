//! Read-only inventory of native mesh programs in the user's own files.
//!
//! The report deliberately contains aggregate shader metadata, not shader
//! bytecode or extracted game assets. It is a planning tool for the
//! compute/vertex fallback; it does not imply that the fallback exists.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::PathBuf;

use anyhow::Result;
use victory_shader::native::StageIo;
use victory_shader::{Container, FourCC, HwStage, native_programs};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct GroupSummary {
    programs: usize,
    wave64: usize,
    with_lds: usize,
    max_lds_bytes: u32,
    max_vertices: u32,
    max_primitives: u32,
}

impl GroupSummary {
    fn record(&mut self, wave64: bool, lds_bytes: u32, max_vertices: u32, max_primitives: u32) {
        self.programs += 1;
        self.wave64 += usize::from(wave64);
        self.with_lds += usize::from(lds_bytes != 0);
        self.max_lds_bytes = self.max_lds_bytes.max(lds_bytes);
        self.max_vertices = self.max_vertices.max(max_vertices);
        self.max_primitives = self.max_primitives.max(max_primitives);
    }

    fn merge(&mut self, other: &Self) {
        self.programs += other.programs;
        self.wave64 += other.wave64;
        self.with_lds += other.with_lds;
        self.max_lds_bytes = self.max_lds_bytes.max(other.max_lds_bytes);
        self.max_vertices = self.max_vertices.max(other.max_vertices);
        self.max_primitives = self.max_primitives.max(other.max_primitives);
    }
}

#[derive(Debug, Default)]
struct Census {
    files_scanned: usize,
    large_files_skipped: usize,
    containers_with_dxil: usize,
    unreadable_native_tables: usize,
    containers_with_mesh: usize,
    groups: BTreeMap<u32, GroupSummary>,
}

impl Census {
    fn report(&self) -> String {
        let mut out = String::new();
        let total = self.groups.values().fold(GroupSummary::default(), |mut sum, row| {
            sum.merge(row);
            sum
        });

        let _ = writeln!(out, "files scanned: {}", self.files_scanned);
        let _ = writeln!(out, "files skipped (over {} MiB): {}", crate::shader::MAX_FILE / (1 << 20), self.large_files_skipped);
        let _ = writeln!(out, "DXIL containers with native tables: {}", self.containers_with_dxil);
        let _ = writeln!(out, "native tables that could not be read: {}", self.unreadable_native_tables);
        let _ = writeln!(out, "containers with mesh programs: {}", self.containers_with_mesh);
        let _ = writeln!(out, "mesh programs: {}", total.programs);
        if total.programs == 0 {
            let _ = writeln!(out, "No native mesh programs were found in the scanned files.");
            return out;
        }

        let _ = writeln!(out);
        let _ = writeln!(out, "| group threads | programs | wave64 | programs using LDS | max LDS bytes | max vertices | max primitives |");
        let _ = writeln!(out, "|---:|---:|---:|---:|---:|---:|---:|");
        for (threads, row) in &self.groups {
            let _ = writeln!(
                out,
                "| {threads} | {} | {} | {} | {} | {} | {} |",
                row.programs,
                row.wave64,
                row.with_lds,
                row.max_lds_bytes,
                row.max_vertices,
                row.max_primitives,
            );
        }
        let _ = writeln!(
            out,
            "| **all** | **{}** | **{}** | **{}** | **{}** | **{}** | **{}** |",
            total.programs,
            total.wave64,
            total.with_lds,
            total.max_lds_bytes,
            total.max_vertices,
            total.max_primitives,
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "Wave64 is the console program's wave requirement; the table does not test host GPU support.");
        let _ = writeln!(out, "This census does not translate shaders, create D3D12 pipelines, or run the game.");
        out
    }
}

/// Scan files or directories for native mesh shader programs and print only
/// aggregate metadata. The --list option adds one numeric line per program.
pub(crate) fn run(paths: Vec<PathBuf>, list: bool) -> Result<()> {
    let paths = crate::default_paths(paths)?;
    let mut census = Census::default();

    for root in &paths {
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if size > crate::shader::MAX_FILE {
                census.large_files_skipped += 1;
                continue;
            }
            let Ok(data) = std::fs::read(entry.path()) else {
                continue;
            };
            census.files_scanned += 1;

            for source in crate::shader::sources(&data) {
                let Some(bytes) = source.bytes() else {
                    continue;
                };
                let Ok(container) = Container::parse(&bytes) else {
                    continue;
                };
                let Some(dxil) = container.part(FourCC::DXIL) else {
                    continue;
                };
                let programs = match native_programs(dxil.data) {
                    Ok(programs) => programs,
                    Err(_) => {
                        census.unreadable_native_tables += 1;
                        continue;
                    }
                };
                census.containers_with_dxil += 1;
                let mut found_mesh = false;

                for (index, program) in programs.iter().enumerate() {
                    if program.stage != HwStage::Ms {
                        continue;
                    }
                    let StageIo::Ms(mesh) = &program.io else {
                        continue;
                    };
                    found_mesh = true;
                    census
                        .groups
                        .entry(mesh.threads)
                        .or_default()
                        .record(
                            program.wave64,
                            mesh.lds_bytes,
                            mesh.max_vertices,
                            mesh.max_primitives,
                        );
                    if list {
                        println!(
                            "{}{}: program={} threads={} wave={} lds={} max_vertices={} max_primitives={} topology={}",
                            entry.path().display(),
                            source.label(),
                            index,
                            mesh.threads,
                            if program.wave64 { 64 } else { 32 },
                            mesh.lds_bytes,
                            mesh.max_vertices,
                            mesh.max_primitives,
                            mesh.topology,
                        );
                    }
                }
                census.containers_with_mesh += usize::from(found_mesh);
            }
        }
    }

    print!("{}", census.report());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_rows_accumulate_wave_lds_and_output_limits() {
        let mut row = GroupSummary::default();
        row.record(true, 512, 64, 96);
        row.record(false, 0, 128, 32);
        assert_eq!(
            row,
            GroupSummary {
                programs: 2,
                wave64: 1,
                with_lds: 1,
                max_lds_bytes: 512,
                max_vertices: 128,
                max_primitives: 96,
            }
        );
    }

    #[test]
    fn report_is_explicit_when_no_mesh_programs_are_found() {
        let report = Census::default().report();
        assert!(report.contains("mesh programs: 0"));
        assert!(report.contains("No native mesh programs were found"));
    }
}
