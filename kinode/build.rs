use std::{
    collections::HashSet,
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};
use zip::write::FileOptions;

fn get_features() -> String {
    let mut features = "".to_string();
    for (key, _) in std::env::vars() {
        if key.starts_with("CARGO_FEATURE_") {
            let feature = key
                .trim_start_matches("CARGO_FEATURE_")
                .to_lowercase()
                .replace("_", "-");
            features.push_str(&feature);
        }
    }
    features
}

fn output_reruns(dir: &Path, rerun_files: &HashSet<String>) {
    // Check files individually
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                // Check if the current file is in our list of interesting files
                if filename == "ui" {
                    continue;
                }
                if rerun_files.contains(filename) {
                    // If so, print a `cargo:rerun-if-changed=PATH` line for it
                    println!("cargo::rerun-if-changed={}", path.display());
                    continue;
                }
            }
            if path.is_dir() {
                // If the entry is a directory not in rerun_files, recursively walk it
                output_reruns(&path, rerun_files);
            }
        }
    }
}

fn build_and_zip_package(
    entry_path: PathBuf,
    parent_pkg_path: &str,
    features: &str,
) -> anyhow::Result<(String, String, Vec<u8>)> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        kit::build::execute(
            &entry_path,
            true,
            false,
            true,
            features,
            None,
            None,
            None,
            true,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;

        let mut writer = Cursor::new(Vec::new());
        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o755);
        {
            let mut zip = zip::ZipWriter::new(&mut writer);

            for sub_entry in walkdir::WalkDir::new(parent_pkg_path) {
                let sub_entry = sub_entry?;
                let path = sub_entry.path();
                let name = path.strip_prefix(Path::new(parent_pkg_path))?;

                if path.is_file() {
                    zip.start_file(name.to_string_lossy(), options)?;
                    let mut file = File::open(path)?;
                    let mut buffer = Vec::new();
                    file.read_to_end(&mut buffer)?;
                    zip.write_all(&buffer)?;
                } else if !name.as_os_str().is_empty() {
                    zip.add_directory(name.to_string_lossy(), options)?;
                }
            }
            zip.finish()?;
        }

        let zip_contents = writer.into_inner();
        let zip_filename = format!("{}.zip", entry_path.file_name().unwrap().to_str().unwrap());
        Ok((entry_path.display().to_string(), zip_filename, zip_contents))
    })
}

fn main() -> anyhow::Result<()> {
    if std::env::var("SKIP_BUILD_SCRIPT").is_ok() {
        println!("Skipping build script");
        return Ok(());
    }

    let pwd = std::env::current_dir()?;
    let parent_dir = pwd.parent().unwrap();
    let packages_dir = pwd.join("packages");

    if std::env::var("SKIP_BUILD_FRONTEND").is_ok() {
        println!("Skipping build frontend");
    } else {
        // build core frontends
        let core_frontends = vec![
            "src/register-ui",
            "packages/app_store/ui",
            "packages/homepage/ui",
            // chess when brought in
        ];

        // for each frontend, execute build.sh
        for frontend in core_frontends {
            let status = std::process::Command::new("sh")
                .current_dir(pwd.join(frontend))
                .arg("./build.sh")
                .status()?;
            if !status.success() {
                return Err(anyhow::anyhow!("Failed to build frontend: {}", frontend));
            }
        }
    }

    let entries: Vec<_> = fs::read_dir(packages_dir)?
        .map(|entry| entry.unwrap().path())
        .collect();

    let rerun_files: HashSet<String> = HashSet::from([
        "Cargo.lock".to_string(),
        "Cargo.toml".to_string(),
        "src".to_string(),
    ]);
    output_reruns(&parent_dir, &rerun_files);

    let features = get_features();

    let results: Vec<anyhow::Result<(String, String, Vec<u8>)>> = entries
        .iter()
        .filter_map(|entry_path| {
            let parent_pkg_path = entry_path.join("pkg");
            if !parent_pkg_path.exists() {
                // don't run on, e.g., `.DS_Store`
                return None;
            }
            Some(build_and_zip_package(
                entry_path.clone(),
                parent_pkg_path.to_str().unwrap(),
                &features,
            ))
        })
        .collect();

    // Process results, e.g., write to `bootstrapped_processes.rs`
    // This part remains sequential
    let mut bootstrapped_processes = vec![];
    writeln!(
        bootstrapped_processes,
        "pub static BOOTSTRAPPED_PROCESSES: &[(&str, &[u8], &[u8])] = &["
    )?;

    for result in results {
        match result {
            Ok((entry_path, zip_filename, zip_contents)) => {
                // Further processing, like saving ZIP files and updating bootstrapped_processes
                let metadata_path = format!("{}/metadata.json", entry_path);
                let zip_path = format!("{}/target/{}", parent_dir.display(), zip_filename);
                fs::write(&zip_path, &zip_contents)?;

                writeln!(
                    bootstrapped_processes,
                    "    (\"{}\", include_bytes!(\"{}\"), include_bytes!(\"{}\")),",
                    zip_filename, metadata_path, zip_path,
                )?;
            }
            Err(e) => return Err(e),
        }
    }

    writeln!(bootstrapped_processes, "];")?;
    let target_dir = pwd.join("../target");
    if !target_dir.exists() {
        fs::create_dir_all(&target_dir)?;
    }
    let bootstrapped_processes_path = target_dir.join("bootstrapped_processes.rs");
    fs::write(&bootstrapped_processes_path, bootstrapped_processes)?;

    Ok(())
}
