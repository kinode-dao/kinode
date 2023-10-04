use std::process::Command;
use std::{fs, io};

fn run_command(cmd: &mut Command) -> io::Result<()> {
    let status = cmd.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, "Command failed"))
    }
}

fn main() {
    if std::env::var("SKIP_BUILD_SCRIPT").is_ok() {
        println!("Skipping build script");
        return;
    }

    let pwd = std::env::current_dir().unwrap();

    // create target.wasm (compiled .wit) & world
    run_command(Command::new("wasm-tools").args(&[
        "component",
        "wit",
        &format!("{}/wit/", pwd.display()),
        "-o",
        "target.wasm",
        "--wasm",
    ]))
    .unwrap();
    run_command(Command::new("touch").args(&[&format!("{}/world", pwd.display())])).unwrap();

    // Build wasm32-wasi apps.
    const WASI_APPS: [&str; 8] = [
        "homepage",
        "chess",
        "http_bindings",
        "http_proxy",
        "orgs",
        "qns_indexer",
        "rpc",
        "terminal",
    ];
    for name in WASI_APPS {
        // only execute if one of the modules has source code changes
        println!("cargo:rerun-if-changed=modules/{}/src", name);
        // copy in the wit files
        run_command(
            Command::new("rm").args(&["-rf", &format!("{}/modules/{}/wit", pwd.display(), name)]),
        )
        .unwrap();
        run_command(Command::new("cp").args(&[
            "-r",
            "wit",
            &format!("{}/modules/{}", pwd.display(), name),
        ]))
        .unwrap();

        fs::create_dir_all(&format!(
            "{}/modules/{}/target/bindings/{}",
            pwd.display(),
            name,
            name
        ))
        .unwrap();
        run_command(Command::new("cp").args(&[
            "target.wasm",
            &format!(
                "{}/modules/{}/target/bindings/{}/",
                pwd.display(),
                name,
                name
            ),
        ]))
        .unwrap();
        run_command(Command::new("cp").args(&[
            "world",
            &format!(
                "{}/modules/{}/target/bindings/{}/",
                pwd.display(),
                name,
                name
            ),
        ]))
        .unwrap();

        fs::create_dir_all(&format!(
            "{}/modules/{}/target/wasm32-unknown-unknown/release",
            pwd.display(),
            name
        ))
        .unwrap();

        // build the module
        run_command(Command::new("cargo").args(&[
            "build",
            "--release",
            "--no-default-features",
            &format!(
                "--manifest-path={}/modules/{}/Cargo.toml",
                pwd.display(),
                name
            ),
            "--target",
            "wasm32-wasi",
        ]))
        .unwrap();

        //  adapt module to component with adaptor
        run_command(Command::new("wasm-tools").args(&[
            "component",
            "new",
            &format!(
                "{}/modules/{}/target/wasm32-wasi/release/{}.wasm",
                pwd.display(),
                name,
                name
            ),
            "-o",
            &format!(
                "{}/modules/{}/target/wasm32-wasi/release/{}_adapted.wasm",
                pwd.display(),
                name,
                name
            ),
            "--adapt",
            &format!("{}/wasi_snapshot_preview1.wasm", pwd.display()),
        ]))
        .unwrap();

        //  put wit into component & place where boot sequence expects to find it
        run_command(Command::new("wasm-tools").args(&[
            "component",
            "embed",
            "wit",
            "--world",
            "uq-process",
            &format!(
                "{}/modules/{}/target/wasm32-wasi/release/{}_adapted.wasm",
                pwd.display(),
                name,
                name
            ),
            "-o",
            &format!(
                "{}/modules/{}/target/wasm32-unknown-unknown/release/{}.wasm",
                pwd.display(),
                name,
                name
            ),
        ]))
        .unwrap();
    }
}
