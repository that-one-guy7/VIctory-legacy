//! `cargo xtask agility`: the D3D12 Agility SDK runtime beside victory.exe.
//!
//! Windows 10's in-box D3D12 stops at shader model 6.5, below what every
//! translated pipeline needs (issue #84). victory.exe asks D3D12 for a newer
//! runtime (`apps/victory/src/agility.rs`); this puts Microsoft's
//! `D3D12Core.dll`, and the debug layer's `d3d12SDKLayers.dll`, where it
//! looks: `<target>/<profile>/D3D12/`.
//!
//! The DLLs are Microsoft's, so they never enter the repository (asset
//! policy): they are fetched from nuget.org, at a pinned version whose
//! SHA-512 nuget.org publishes, and kept under the target folder.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha512};

/// The package version. Its minor number is `D3D12SDKVersion` in
/// `apps/victory/src/agility.rs`: keep the two in step.
const VERSION: &str = "1.620.2";
/// The package's SHA-512, as nuget.org's catalog gives it (base64
/// `1oP/f1cm…MnNQ==`).
const SHA512: &str = "d683ff7f5726906c3f43d299f6cdd4b1cad22f8329ccd27b3492d0a1413d9904\
                      102b8d543419bec3b3545c86bf49f6a500ebf42d95c999aa0969e5d693032735";
/// What we take from the package.
const FILES: &[&str] = &[
    "build/native/bin/x64/D3D12Core.dll",
    "build/native/bin/x64/d3d12SDKLayers.dll",
];
/// The profiles victory.exe is built with (README).
const PROFILES: &[&str] = &["debug", "release", "diag"];

pub fn run(target: &Path) -> Result<()> {
    let package = fetch(target)?;
    let files = FILES
        .iter()
        .map(|name| Ok((*name, extract(&package, name)?)))
        .collect::<Result<Vec<_>>>()?;
    for profile in PROFILES {
        let dir = target.join(profile).join("D3D12");
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        for (name, bytes) in &files {
            let file = dir.join(name.rsplit('/').next().unwrap_or(name));
            std::fs::write(&file, bytes).with_context(|| format!("writing {}", file.display()))?;
        }
        println!("D3D12 Agility SDK {VERSION}: {}", dir.display());
    }
    Ok(())
}

/// The package's bytes, downloaded once into `<target>/agility/`.
fn fetch(target: &Path) -> Result<Vec<u8>> {
    let dir = target.join("agility");
    let file = dir.join(format!("microsoft.direct3d.d3d12.{VERSION}.nupkg"));
    if let Ok(bytes) = std::fs::read(&file)
        && hex(&Sha512::digest(&bytes)) == SHA512
    {
        return Ok(bytes);
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let url = format!(
        "https://api.nuget.org/v3-flatcontainer/microsoft.direct3d.d3d12/{VERSION}/\
         microsoft.direct3d.d3d12.{VERSION}.nupkg"
    );
    let partial = file.with_extension("partial");
    println!("downloading {url}");
    // curl ships with Windows 10 (1803) and later.
    let status = Command::new("curl")
        .args(["--fail", "--location", "--silent", "--show-error"])
        .args(["--retry", "3", "--output"])
        .arg(&partial)
        .arg(&url)
        .status()
        .context("running curl")?;
    ensure!(status.success(), "curl could not download {url}");
    let bytes = std::fs::read(&partial)?;
    let got = hex(&Sha512::digest(&bytes));
    if got != SHA512 {
        let _ = std::fs::remove_file(&partial);
        bail!("{url}: SHA-512 is {got}, expected {SHA512}");
    }
    std::fs::rename(&partial, &file)?;
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn u16_at(b: &[u8], at: usize) -> Result<usize> {
    let s = b.get(at..at + 2).context("truncated zip")?;
    Ok(u16::from_le_bytes([s[0], s[1]]).into())
}

fn u32_at(b: &[u8], at: usize) -> Result<usize> {
    let s = b.get(at..at + 4).context("truncated zip")?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

/// One file of a zip (a .nupkg is one): found through the central
/// directory, stored or deflated. No zip64; this package is 35 MB.
fn extract(zip: &[u8], name: &str) -> Result<Vec<u8>> {
    const END: u32 = 0x0605_4b50;
    const CENTRAL: usize = 0x0201_4b50;
    const LOCAL: usize = 0x0403_4b50;
    let end = (0..zip.len().saturating_sub(21))
        .rev()
        .take(0x1_0000 + 22)
        .find(|&at| zip[at..].starts_with(&END.to_le_bytes()))
        .context("no zip end record")?;
    let entries = u16_at(zip, end + 10)?;
    let mut at = u32_at(zip, end + 16)?;
    for _ in 0..entries {
        ensure!(u32_at(zip, at)? == CENTRAL, "bad zip central directory");
        let method = u16_at(zip, at + 10)?;
        let packed = u32_at(zip, at + 20)?;
        let size = u32_at(zip, at + 24)?;
        let name_len = u16_at(zip, at + 28)?;
        let skip = name_len + u16_at(zip, at + 30)? + u16_at(zip, at + 32)?;
        let local = u32_at(zip, at + 42)?;
        let this = zip
            .get(at + 46..at + 46 + name_len)
            .context("truncated zip")?;
        at += 46 + skip;
        if this != name.as_bytes() {
            continue;
        }
        ensure!(u32_at(zip, local)? == LOCAL, "bad zip local header");
        let data = local + 30 + u16_at(zip, local + 26)? + u16_at(zip, local + 28)?;
        let data = zip.get(data..data + packed).context("truncated zip")?;
        let out = match method {
            0 => data.to_vec(),
            8 => miniz_oxide::inflate::decompress_to_vec(data)
                .map_err(|e| anyhow::anyhow!("{name}: inflate failed: {e:?}"))?,
            m => bail!("{name}: zip method {m} is not supported"),
        };
        ensure!(
            out.len() == size,
            "{name}: {} bytes, expected {size}",
            out.len()
        );
        return Ok(out);
    }
    bail!("{name} is not in the package")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A zip with one stored and one deflated file, written here.
    fn zip(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
        let (mut out, mut central) = (Vec::new(), Vec::new());
        for (name, data, deflate) in files {
            let packed = if *deflate {
                miniz_oxide::deflate::compress_to_vec(data, 6)
            } else {
                data.to_vec()
            };
            let method: u16 = if *deflate { 8 } else { 0 };
            let local = out.len() as u32;
            out.extend(0x0403_4b50_u32.to_le_bytes());
            out.extend([0; 4]);
            out.extend(method.to_le_bytes());
            out.extend([0; 8]);
            out.extend((packed.len() as u32).to_le_bytes());
            out.extend((data.len() as u32).to_le_bytes());
            out.extend((name.len() as u16).to_le_bytes());
            out.extend([0; 2]);
            out.extend(name.as_bytes());
            out.extend(&packed);
            central.extend(0x0201_4b50_u32.to_le_bytes());
            central.extend([0; 6]);
            central.extend(method.to_le_bytes());
            central.extend([0; 8]);
            central.extend((packed.len() as u32).to_le_bytes());
            central.extend((data.len() as u32).to_le_bytes());
            central.extend((name.len() as u16).to_le_bytes());
            central.extend([0; 12]);
            central.extend(local.to_le_bytes());
            central.extend(name.as_bytes());
        }
        let start = out.len() as u32;
        let count = (files.len() as u16).to_le_bytes();
        out.extend(&central);
        out.extend(0x0605_4b50_u32.to_le_bytes());
        out.extend([0; 4]);
        out.extend(count);
        out.extend(count);
        out.extend((central.len() as u32).to_le_bytes());
        out.extend(start.to_le_bytes());
        out.extend([0; 2]);
        out
    }

    #[test]
    fn extracts_stored_and_deflated_files() {
        let text = b"synthetic data, repeated, repeated, repeated".repeat(20);
        let z = zip(&[
            ("a/stored.txt", b"plain", false),
            ("b/packed.txt", &text, true),
        ]);
        assert_eq!(extract(&z, "a/stored.txt").unwrap(), b"plain");
        assert_eq!(extract(&z, "b/packed.txt").unwrap(), text);
        assert!(extract(&z, "missing").is_err());
    }

    #[test]
    fn version_matches_the_sdk_version_victory_exports() {
        let source = include_str!("../../apps/victory/src/agility.rs");
        let minor = VERSION.split('.').nth(1).unwrap();
        assert!(
            source.contains(&format!("D3D12SDKVersion: u32 = {minor};")),
            "apps/victory/src/agility.rs must export D3D12SDKVersion = {minor}"
        );
    }
}
