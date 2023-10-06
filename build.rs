use std::process::Command;
use std::{
    fs, io,
    io::{Read, Write},
};

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
    // only execute if one of the modules has source code changes
    const WASI_APPS: [&str; 9] = [
        "app_tracker",
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
        println!("cargo:rerun-if-changed=modules/{}/src", name);
        println!("cargo:rerun-if-changed=modules/{}/pkg/manifest.json", name);
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
    for name in WASI_APPS {
        // remove old wit, if it existed
        run_command(
            Command::new("rm").args(&["-rf", &format!("{}/modules/{}/wit", pwd.display(), name)]),
        )
        .unwrap();
        // copy in newly-made wit
        run_command(Command::new("cp").args(&[
            "-r",
            "wit",
            &format!("{}/modules/{}", pwd.display(), name),
        ]))
        .unwrap();
        // create target/bindings directory
        fs::create_dir_all(&format!(
            "{}/modules/{}/target/bindings/{}",
            pwd.display(),
            name,
            name
        ))
        .unwrap();
        // copy newly-made target.wasm into target/bindings
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
        // copy newly-made world into target/bindings
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
        // build the module targeting wasm32-wasi
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
        //  adapt module to component with adapter based on wasi_snapshot_preview1.wasm
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
        //  put wit into component & place final .wasm in /pkg
        let pkg_folder = format!("{}/modules/{}/pkg/", pwd.display(), name);
        let _ = run_command(Command::new("mkdir").args(&["-p", &pkg_folder]));
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
            &format!("{}/{}.wasm", pkg_folder, name),
        ]))
        .unwrap();
        // from the pkg folder, create a zip archive and save in target directory
        let writer =
            std::fs::File::create(format!("{}/target/{}.zip", pwd.display(), name)).unwrap();
        let options = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored) // or CompressionMethod::Deflated
            .unix_permissions(0o755);
        let mut zip = zip::ZipWriter::new(writer);

        for entry in walkdir::WalkDir::new(&pkg_folder) {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = path
                .strip_prefix(std::path::Path::new(&pkg_folder))
                .unwrap();

            // Write a directory or file to the ZIP archive
            if path.is_file() {
                zip.start_file(name.to_string_lossy().into_owned(), options)
                    .unwrap();
                let mut file = std::fs::File::open(path).unwrap();
                let mut buffer = Vec::new();
                file.read_to_end(&mut buffer).unwrap();
                zip.write_all(&buffer).unwrap();
            } else if name.as_os_str().len() != 0 {
                zip.add_directory(name.to_string_lossy().into_owned(), options)
                    .unwrap();
            }
        }
        zip.finish().unwrap();
    }
}
