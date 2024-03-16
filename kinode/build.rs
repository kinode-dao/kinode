use rayon::prelude::*;
use std::{
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
            //println!("cargo:rustc-cfg=feature=\"{}\"", feature);
            //println!("- {}", feature);
        }
    }
    features
}

fn build_and_zip_package(
    entry_path: PathBuf,
    parent_pkg_path: &str,
    features: &str,
) -> anyhow::Result<(String, String, Vec<u8>)> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        kit::build::execute(&entry_path, false, false, true, features).await?;

        let mut writer = Cursor::new(Vec::new());
        let options = FileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
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

    let entries: Vec<_> = fs::read_dir(packages_dir)?
        .map(|entry| entry.unwrap().path())
        .collect();

    let features = get_features();

    let results: Vec<anyhow::Result<(String, String, Vec<u8>)>> = entries
        .par_iter()
        .map(|entry_path| {
            let parent_pkg_path = entry_path.join("pkg");
            build_and_zip_package(
                entry_path.clone(),
                parent_pkg_path.to_str().unwrap(),
                &features,
            )
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
    let bootstrapped_processes_path = pwd.join("src/bootstrapped_processes.rs");
    fs::write(&bootstrapped_processes_path, bootstrapped_processes)?;

    Ok(())
}
