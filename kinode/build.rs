use std::{fs, io::{Read, Write}};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var("SKIP_BUILD_SCRIPT").is_ok() {
        println!("Skipping build script");
        return Ok(());
    }

    let pwd = std::env::current_dir().unwrap();
    let parent_dir = pwd.parent().unwrap();

    // Build wasm32-wasi apps, zip, and add to bootstrapped_processes.rs
    let mut bootstrapped_processes =
        fs::File::create(format!("{}/src/bootstrapped_processes.rs", pwd.display(),)).unwrap();
    writeln!(
        bootstrapped_processes,
        "pub static BOOTSTRAPPED_PROCESSES: &[(&str, &[u8])] = &[",
    )
    .unwrap();
    let packages_dir = format!("{}/packages", pwd.display());
    eprintln!("{packages_dir:?}");
    for entry in std::fs::read_dir(packages_dir).unwrap() {
        let entry_path = entry.unwrap().path();
        let parent_pkg_path = format!("{}/pkg", entry_path.display());

        kit::build::execute(&entry_path, false, false, false, true).await?;

        // After processing all sub-apps, zip the parent's pkg/ directory
        let zip_filename = format!("{}.zip", entry_path.file_name().unwrap().to_str().unwrap(),);
        let zip_path = format!("{}/target/{}", parent_dir.display(), zip_filename);
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
    Ok(())
}
