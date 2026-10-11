use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=resources/windows/Subroutine.rc");
    println!("cargo:rerun-if-changed=resources/windows/Subroutine.ico");
    for name in [
        "RC",
        "INCLUDE",
        "WindowsSdkDir",
        "WindowsSDKVersion",
        "VCToolsVersion",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
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
    // SDK tools need their include paths even when rc.exe is already on PATH.
    let tool = if msvc {
        find_msvc_tools::find_tool(&env::var("TARGET").unwrap(), "rc.exe")
    } else {
        None
    };
    let compiler = env::var_os("RC")
        .or_else(|| tool.as_ref().map(|tool| tool.path().as_os_str().to_owned()))
        .unwrap_or_else(|| if msvc { "rc.exe" } else { "windres" }.into());
    let resource = out.join(if msvc {
        "Subroutine.res"
    } else {
        "Subroutine.o"
    });
    let mut command = Command::new(&compiler);
    if let Some(tool) = tool {
        command.envs(tool.env().into_iter().cloned());
    }
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
        "Subroutine Lite Windows resource compilation failed; install Visual Studio C++ tools and the Windows SDK, or use a Windows SDK developer shell"
    );
    println!(
        "cargo:rustc-link-arg-bin=subroutine-lite={}",
        resource.display()
    );
}
