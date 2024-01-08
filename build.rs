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

    let start = std::time::Instant::now();

    // if and only if module's wit is outdated, re-set-up build environment
    if file_outdated(
        format!("{}/target.wasm", pwd.display()),
        format!("{}/target/bindings/{}/target.wasm", target_path, name),
    )
    .unwrap_or(true)
    {
        // create target/bindings directory
        fs::create_dir_all(format!("{}/target/bindings/{}", target_path, name,)).unwrap();
        // copy newly-made target.wasm into target/bindings
        run_command(Command::new("cp").args([
            "target.wasm",
            &format!("{}/target/bindings/{}/", target_path, name,),
        ]))
        .unwrap();
        // copy newly-made world into target/bindings
        run_command(Command::new("cp").args([
            "world",
            &format!("{}/target/bindings/{}/", target_path, name,),
        ]))
        .unwrap();
    }
    // Build the module targeting wasm32-wasi
    let bash_build_path = &format!("{}/build.sh", target_path);
    if std::path::Path::new(&bash_build_path).exists() {
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(target_path).unwrap();
        run_command(Command::new("/bin/bash").arg("build.sh")).unwrap();
        std::env::set_current_dir(cwd).unwrap();
    } else {
        run_command(Command::new("cargo").args([
            "+nightly",
            "build",
            "--release",
            "--no-default-features",
            &format!("--manifest-path={}/Cargo.toml", target_path),
            "--target",
            "wasm32-wasi",
        ]))
        .unwrap();
    }
    // Adapt module to component with adapter based on wasi_snapshot_preview1.wasm
    run_command(Command::new("wasm-tools").args([
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
        let _ = run_command(Command::new("mkdir").args(["-p", &pkg_folder]));
        format!("{}/{}.wasm", pkg_folder, name)
    };

    // Embed "wit" into the component
    run_command(Command::new("wasm-tools").args([
        "component",
        "embed",
        "wit",
        "--world",
        "process",
        &format!(
            "{}/target/wasm32-wasi/release/{}_adapted.wasm",
            target_path, name
        ),
        "-o",
        &wasm_dest_path,
    ]))
    .unwrap();

    let end = std::time::Instant::now();
    println!(
        "cargo:warning=building {} took {:?}",
        target_path,
        end.duration_since(start)
    );
}

fn main() {
    if std::env::var("SKIP_BUILD_SCRIPT").is_ok() {
        println!("Skipping build script");
        return;
    }

    let pwd = std::env::current_dir().unwrap();

    // Pull wit from git repo
    let wit_dir = pwd.join("wit");
    fs::create_dir_all(&wit_dir).unwrap();
    let wit_file = wit_dir.join("nectar.wit");
    //if !wit_file.exists() { // TODO: cache in better way
    let mut wit_file = std::fs::File::create(&wit_file).unwrap();
    let nectar_wit_url = "https://raw.githubusercontent.com/uqbar-dao/uqwit/master/nectar.wit";
    let mut response = reqwest::blocking::get(nectar_wit_url).unwrap();
    io::copy(&mut response, &mut wit_file).unwrap();

    // Create target.wasm (compiled .wit) & world
    run_command(Command::new("wasm-tools").args([
        "component",
        "wit",
        &format!("{}/wit/", pwd.display()),
        "-o",
        "target.wasm",
        "--wasm",
    ]))
    .unwrap();
    run_command(Command::new("touch").args([&format!("{}/world", pwd.display())])).unwrap();

    // Build wasm32-wasi apps, zip, and add to bootstrapped_processes.rs
    let mut bootstrapped_processes =
        fs::File::create(format!("{}/src/bootstrapped_processes.rs", pwd.display(),)).unwrap();
    writeln!(
        bootstrapped_processes,
        "pub static BOOTSTRAPPED_PROCESSES: &[(&str, &'static [u8])] = &[",
    )
    .unwrap();
    let modules_dir = format!("{}/modules", pwd.display());
    for entry in std::fs::read_dir(modules_dir).unwrap() {
        let entry_path = entry.unwrap().path();

        // Build the app
        let parent_pkg_path = format!("{}/pkg", entry_path.display());
        fs::create_dir_all(&parent_pkg_path).unwrap();

        // Otherwise, consider it a directory containing subdirectories with potential apps
        for sub_entry in std::fs::read_dir(&entry_path).unwrap() {
            let sub_entry_path = sub_entry.unwrap().path();
            if sub_entry_path.join("Cargo.toml").exists() {
                build_app(
                    &sub_entry_path.display().to_string(),
                    sub_entry_path.file_name().unwrap().to_str().unwrap(),
                    Some(&parent_pkg_path),
                );
            }
        }

        // After processing all sub-apps, zip the parent's pkg/ directory
        let zip_filename = format!("{}.zip", entry_path.file_name().unwrap().to_str().unwrap(),);
        let zip_path = format!("{}/target/{}", pwd.display(), zip_filename,);
        let writer = std::fs::File::create(&zip_path).unwrap();
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
            } else if !name.as_os_str().is_empty() {
                zip.add_directory(name.to_string_lossy().into_owned(), options)
                    .unwrap();
            }
        }
        zip.finish().unwrap();

        // Add zip bytes to bootstrapped_processes.rs
        writeln!(
            bootstrapped_processes,
            "    (\"{}\", include_bytes!(\"{}\")),",
            zip_filename, zip_path,
        )
        .unwrap();
    }
    writeln!(bootstrapped_processes, "];").unwrap();
}
