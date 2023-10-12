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

fn file_outdated<P1, P2>(input: P1, output: P2) -> io::Result<bool>
where
    P1: AsRef<std::path::Path>,
    P2: AsRef<std::path::Path>,
{
    let out_meta = std::fs::metadata(output);
    if let Ok(meta) = out_meta {
        let output_mtime = meta.modified()?;

        // if input file is more recent than our output, we are outdated
        let input_meta = fs::metadata(input)?;
        let input_mtime = input_meta.modified()?;

        Ok(input_mtime > output_mtime)
    } else {
        // output file not found, we are outdated
        Ok(true)
    }
}

fn build_app(target_path: &str, name: &str, parent_pkg_path: Option<&str>) {
    let pwd = std::env::current_dir().unwrap();

    // Copy in newly-made wit IF old one is outdated
    if file_outdated(
        format!("{}/wit/", pwd.display()),
        format!("{}/modules/{}/wit/", target_path, name),
    )
    .unwrap_or(true)
    {
        run_command(Command::new("cp").args(&["-r", "wit", target_path])).unwrap();
        // create target/bindings directory
        fs::create_dir_all(&format!("{}/target/bindings/{}", target_path, name,)).unwrap();
        // copy newly-made target.wasm into target/bindings
        run_command(Command::new("cp").args(&[
            "target.wasm",
            &format!("{}/target/bindings/{}/", target_path, name,),
        ]))
        .unwrap();
        // copy newly-made world into target/bindings
        run_command(Command::new("cp").args(&[
            "world",
            &format!("{}/target/bindings/{}/", target_path, name,),
        ]))
        .unwrap();
    }
    // Build the module targeting wasm32-wasi
    run_command(Command::new("cargo").args(&[
        "build",
        "--release",
        "--no-default-features",
        &format!("--manifest-path={}/Cargo.toml", target_path),
        "--target",
        "wasm32-wasi",
    ]))
    .unwrap();
    // Adapt module to component with adapter based on wasi_snapshot_preview1.wasm
    run_command(Command::new("wasm-tools").args(&[
        "component",
        "new",
        &format!("{}/target/wasm32-wasi/release/{}.wasm", target_path, name),
        "-o",
        &format!(
            "{}/target/wasm32-wasi/release/{}_adapted.wasm",
            target_path, name
        ),
        "--adapt",
        &format!("{}/wasi_snapshot_preview1.wasm", pwd.display()),
    ]))
    .unwrap();

    // Determine the destination for the .wasm file after embedding wit
    let wasm_dest_path = if let Some(parent_pkg) = parent_pkg_path {
        format!("{}/{}.wasm", parent_pkg, name)
    } else {
        let pkg_folder = format!("{}/pkg/", target_path);
        let _ = run_command(Command::new("mkdir").args(&["-p", &pkg_folder]));
        format!("{}/{}.wasm", pkg_folder, name)
    };

    // Embed "wit" into the component
    run_command(Command::new("wasm-tools").args(&[
        "component",
        "embed",
        "wit",
        "--world",
        "uq-process",
        &format!(
            "{}/target/wasm32-wasi/release/{}_adapted.wasm",
            target_path, name
        ),
        "-o",
        &wasm_dest_path,
    ]))
    .unwrap();
}

fn main() {
    if std::env::var("SKIP_BUILD_SCRIPT").is_ok() {
        println!("Skipping build script");
        return;
    }
    let build_enabled = std::env::var("BUILD_APPS")
        .map(|v| v == "true")
        .unwrap_or(true); // run by default

    if !build_enabled {
        return;
    }
    // only execute if one of the modules has source code changes
    const WASI_APPS: [&str; 9] = [
        "app_tracker",
        "chess",
        "homepage",
        "http_bindings",
        "http_proxy",
        "orgs",
        "qns_indexer",
        "rpc",
        "terminal",
    ];
    // NOT YET building KV, waiting for deps to be ready
    const NESTED_WASI_APPS: [(&str, &str); 2] = [
        ("key_value", "key_value"),
        ("key_value", "key_value_worker"),
    ];

    if std::env::var("REBUILD_ALL").is_ok() {
    } else {
        for name in &WASI_APPS {
            println!("cargo:rerun-if-changed=modules/{}/src", name);
            println!("cargo:rerun-if-changed=modules/{}/Cargo.toml", name);
            println!("cargo:rerun-if-changed=modules/{}/pkg/manifest.json", name);
            println!("cargo:rerun-if-changed=modules/{}/pkg/metadata.json", name);
        }
        for (outer, inner) in &NESTED_WASI_APPS {
            println!("cargo:rerun-if-changed=modules/{}/{}/src", outer, inner);
            println!(
                "cargo:rerun-if-changed=modules/{}/{}/Cargo.toml",
                outer, inner
            );
            println!("cargo:rerun-if-changed=modules/{}/pkg/manifest.json", outer);
            println!("cargo:rerun-if-changed=modules/{}/pkg/metadata.json", outer);
        }
    }

    let pwd = std::env::current_dir().unwrap();
    // Create target.wasm (compiled .wit) & world
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
    let modules_dir = format!("{}/modules", pwd.display());
    for entry in std::fs::read_dir(&modules_dir).unwrap() {
        let entry_path = entry.unwrap().path();
        let package_name = entry_path.file_name().unwrap().to_str().unwrap();
        // NOT YET building KV, waiting for deps to be ready
        if package_name == "key_value" {
            return;
        }

        // If Cargo.toml is present, build the app
        let parent_pkg_path = format!("{}/pkg", entry_path.display());
        if entry_path.join("Cargo.toml").exists() {
            build_app(
                &entry_path.display().to_string(),
                &package_name,
                None,
            );
        } else if entry_path.is_dir() {
            fs::create_dir_all(&parent_pkg_path).unwrap();

            // Otherwise, consider it a directory containing subdirectories with potential apps
            for sub_entry in std::fs::read_dir(&entry_path).unwrap() {
                let sub_entry_path = sub_entry.unwrap().path();
                if sub_entry_path.join("Cargo.toml").exists() {
                    build_app(
                        &sub_entry_path.display().to_string(),
                        &sub_entry_path.file_name().unwrap().to_str().unwrap(),
                        Some(&parent_pkg_path),
                    );
                }
            }
        }

        // After processing all sub-apps, zip the parent's pkg/ directory
        let writer = std::fs::File::create(format!(
            "{}/target/{}.zip",
            pwd.display(),
            entry_path.file_name().unwrap().to_str().unwrap()
        ))
        .unwrap();
        let options = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .unix_permissions(0o755);
        let mut zip = zip::ZipWriter::new(writer);
        for sub_entry in walkdir::WalkDir::new(&parent_pkg_path) {
            let sub_entry = sub_entry.unwrap();
            let path = sub_entry.path();
            let name = path
                .strip_prefix(std::path::Path::new(&parent_pkg_path))
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
