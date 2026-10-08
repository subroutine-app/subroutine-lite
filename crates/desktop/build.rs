use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=resources/windows/Subroutine.rc");
    println!("cargo:rerun-if-changed=resources/windows/Subroutine.ico");
    println!("cargo:rerun-if-env-changed=RC");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let resources =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("resources/windows");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let numeric_version = ["MAJOR", "MINOR", "PATCH"]
        .map(|part| env::var(format!("CARGO_PKG_VERSION_{part}")).unwrap())
        .join(",");
    fs::write(
        out.join("version.h"),
        format!(
            "#define LITE_VERSION_NUMERIC {numeric_version},0\n#define LITE_VERSION_STRING \"{version}\"\n"
        ),
    )
    .expect("write Windows version resource header");
    let msvc = env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let compiler =
        env::var_os("RC").unwrap_or_else(|| if msvc { "rc.exe" } else { "windres" }.into());
    let resource = out.join(if msvc {
        "Subroutine.res"
    } else {
        "Subroutine.o"
    });
    let mut command = Command::new(&compiler);
    command.current_dir(&resources);
    if msvc {
        command
            .args(["/nologo", "/I"])
            .arg(&out)
            .arg("/fo")
            .arg(&resource)
            .arg("Subroutine.rc");
    } else {
        command
            .arg("-I")
            .arg(&out)
            .args(["-i", "Subroutine.rc", "-O", "coff", "-o"])
            .arg(&resource);
    }
    let status = command.status().unwrap_or_else(|error| {
        panic!("cannot run {compiler:?}: {error}; use a Windows SDK developer shell or set RC to a target-compatible resource compiler")
    });
    assert!(
        status.success(),
        "Subroutine Lite Windows resource compilation failed"
    );
    println!(
        "cargo:rustc-link-arg-bin=subroutine-lite={}",
        resource.display()
    );
}
