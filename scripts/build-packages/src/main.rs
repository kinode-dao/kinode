use std::{
    collections::HashSet,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use clap::{Arg, Command};
use fs_err as fs;
use zip::write::FileOptions;

fn zip_directory(dir_path: &Path) -> anyhow::Result<Vec<u8>> {
    let mut writer = Cursor::new(Vec::new());
    let options = FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755)
        .last_modified_time(zip::DateTime::from_date_and_time(2023, 6, 19, 0, 0, 0).unwrap());
    {
        let mut zip = zip::ZipWriter::new(&mut writer);

        for sub_entry in walkdir::WalkDir::new(dir_path) {
            let sub_entry = sub_entry?;
            let path = sub_entry.path();
            let name = path.strip_prefix(dir_path)?;

            if path.is_file() {
                zip.start_file(name.to_string_lossy(), options)?;
                let mut file = fs::File::open(path)?;
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
    Ok(zip_contents)
}

fn build_and_zip_package(
    entry_path: PathBuf,
    parent_pkg_path: &str,
    skip_frontend: bool,
    features: &str,
) -> anyhow::Result<(PathBuf, String, Vec<u8>)> {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        kit::build::execute(
            &entry_path,
            skip_frontend,
            false,
            &HashSet::new(),
            &HashSet::new(),
            true,
            features,
            None,
            None,
            None,
            vec![],
            vec![],
            false,
            false,
            false,
            false,
            false,
        )
        .await
        .map_err(|e| anyhow::anyhow!("{:?}", e))?;

        let zip_contents = zip_directory(&Path::new(parent_pkg_path))?;
        let zip_filename = format!("{}.zip", entry_path.file_name().unwrap().to_str().unwrap());
        Ok((entry_path, zip_filename, zip_contents))
    })
}

fn main() -> anyhow::Result<()> {
    let matches = Command::new("build-packages")
        .about("Build the hyperdrive packages.")
        .arg(
            Arg::new("FEATURES")
                .long("features")
                .help("List of features to compile packages with")
                .action(clap::ArgAction::Append),
        )
        .arg(
            Arg::new("SKIP_FRONTEND")
                .long("skip-build-frontend")
                .help("Skip building the frontend")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("OUTPUT_FILENAME")
                .long("output-filename")
                .help("Set output filename (default: packages-{features}.zip)")
                .action(clap::ArgAction::Set),
        )
        .get_matches();

    // hyperdrive/target/debug/build-package
    let current_exe_dir = std::env::current_exe() // build-package
        .unwrap();
    let top_level_dir = current_exe_dir
        .parent() // debug/
        .unwrap()
        .parent() // target/
        .unwrap()
        .parent() // hyperdrive/
        .unwrap();
    let hyperdrive_dir = top_level_dir.join("hyperdrive");
    let packages_dir = hyperdrive_dir.join("packages");

    let mut features = matches
        .get_many::<String>("FEATURES")
        .unwrap_or_default()
        .map(|s| s.to_owned())
        .collect::<Vec<String>>();
    features.sort();
    let features = features.join(",");

    let skip_frontend = matches.get_flag("SKIP_FRONTEND");

    let results: Vec<anyhow::Result<(PathBuf, String, Vec<u8>)>> = fs::read_dir(&packages_dir)?
        .filter_map(|entry| {
            let entry_path = match entry {
                Ok(e) => e.path(),
                Err(_) => return None,
            };
            let child_pkg_path = entry_path.join("pkg");
            if !child_pkg_path.exists() {
                // don't run on, e.g., `.DS_Store`
                return None;
            }
            Some(build_and_zip_package(
                entry_path.clone(),
                child_pkg_path.to_str().unwrap(),
                skip_frontend,
                &features,
            ))
        })
        .collect();

    let mut file_to_metadata = std::collections::HashMap::new();

    let target_dir = top_level_dir.join("target");
    let target_packages_dir = target_dir.join("packages");
    // fresh
    if target_packages_dir.exists() {
        fs::remove_dir_all(&target_packages_dir)?;
    }
    fs::create_dir_all(&target_packages_dir)?;

    for result in results {
        match result {
            Ok((entry_path, zip_filename, zip_contents)) => {
                let metadata_path = entry_path.join("metadata.json");
                let metadata_contents = fs::read_to_string(&metadata_path)?;
                let metadata_contents: serde_json::Value =
                    serde_json::from_str(&metadata_contents)?;
                file_to_metadata.insert(zip_filename.clone(), metadata_contents);
                let zip_path = target_packages_dir.join(&zip_filename);
                fs::write(&zip_path, &zip_contents)?;
            }
            Err(e) => return Err(anyhow::anyhow!("{e:?}")),
        }
    }

    let file_to_metadata = serde_json::to_value(&file_to_metadata)?;
    let file_to_metadata = serde_json::to_string_pretty(&file_to_metadata)?;
    let file_to_metadata_path = target_packages_dir.join("file_to_metadata.json");
    fs::write(&file_to_metadata_path, file_to_metadata)?;

    let package_zip_file_name = match matches.get_one::<String>("OUTPUT_FILENAME") {
        Some(filename) => filename.to_string(),
        None => {
            if features.is_empty() {
                "packages.zip".to_string()
            } else {
                format!("packages-{features}.zip")
            }
        }
    };
    let package_zip_path = target_dir.join(package_zip_file_name);
    let package_zip_contents = zip_directory(&target_packages_dir)?;
    fs::write(package_zip_path, package_zip_contents)?;

    Ok(())
}
