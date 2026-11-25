use std::path::PathBuf;

const COMMON_CONTROLS_V6_MANIFEST: &str = r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
"#;

fn main() {
    let attributes = tauri_build::Attributes::new()
        .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());

    if let Err(error) = tauri_build::try_build(attributes) {
        let error = format!("{error:#}");
        println!("{error}");
        std::process::exit(1);
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is not set"));
    let manifest_path = out_dir.join("app-manifest.xml");
    std::fs::write(&manifest_path, COMMON_CONTROLS_V6_MANIFEST)
        .expect("failed to write the Windows application manifest");

    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!(
        "cargo:rustc-link-arg=/MANIFESTINPUT:{}",
        manifest_path.display()
    );
}
