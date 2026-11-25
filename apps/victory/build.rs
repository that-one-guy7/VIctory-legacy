fn main() {
    // The D3D12 Agility SDK exports (src/agility.rs): D3D12 finds them on
    // the executable. Only the MSVC linker takes /EXPORT on a binary.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/EXPORT:D3D12SDKVersion,DATA");
        println!("cargo:rustc-link-arg-bins=/EXPORT:D3D12SDKPath,DATA");
    }
}
