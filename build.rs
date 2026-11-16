//! Embed the Windows shell icon without introducing a runtime dependency.
use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=assets/brand/unbound.ico");
    println!("cargo:rerun-if-env-changed=RC");
    println!("cargo:rerun-if-env-changed=WINDRES");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest"))
        .join("assets/brand/unbound.ico");
    let rc = out.join("unbound.rc");
    fs::write(
        &rc,
        format!("1 ICON \"{}\"\n", icon.to_string_lossy().replace('\\', "/")),
    )
    .expect("write icon resource");
    let gnu = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("gnu");
    let resource = out.join(if gnu {
        "unbound-icon.o"
    } else {
        "unbound-icon.res"
    });
    let mut command = if gnu {
        let mut c = Command::new(
            env::var_os("WINDRES")
                .or_else(|| env::var_os("RC"))
                .unwrap_or_else(|| "windres".into()),
        );
        c.arg("--input")
            .arg(&rc)
            .arg("--output-format=coff")
            .arg("--output")
            .arg(&resource);
        c
    } else {
        let tool = env::var_os("RC")
            .map(PathBuf::from)
            .or_else(windows_sdk_rc)
            .unwrap_or_else(|| "rc.exe".into());
        let mut c = Command::new(tool);
        c.arg("/nologo").arg("/fo").arg(&resource).arg(&rc);
        c
    };
    let output = command
        .output()
        .expect("Windows icon compiler unavailable: install MinGW windres or Windows SDK rc.exe");
    assert!(
        output.status.success(),
        "Icon resource compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    println!(
        "cargo:rustc-link-arg-bin=apple-toolkit={}",
        resource.display()
    );
}
fn windows_sdk_rc() -> Option<PathBuf> {
    let base = PathBuf::from(env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
    let mut candidates: Vec<_> = fs::read_dir(base)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path().join("x64/rc.exe"))
        .filter(|p| p.is_file())
        .collect();
    candidates.sort();
    candidates.pop()
}
