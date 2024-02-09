use std::{
    fs,
    io::{Cursor, Read, Write},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::var("SKIP_BUILD_SCRIPT").is_ok() {
        println!("Skipping build script");
        return Ok(());
    }

    let pwd = std::env::current_dir().unwrap();
    let parent_dir = pwd.parent().unwrap();

    // Build wasm32-wasi apps, zip, and add to bootstrapped_processes.rs
    let mut bootstrapped_processes = Vec::new();
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
        let mut writer = Cursor::new(Vec::new());
        {
            let options = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(0o755);
            let mut zip = zip::ZipWriter::new(&mut writer);
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
        }
        let zip_contents = writer.into_inner();
        let zip_filename = format!("{}.zip", entry_path.file_name().unwrap().to_str().unwrap(),);
        let zip_path = format!("{}/target/{}", parent_dir.display(), zip_filename);
        if !std::path::Path::new(&zip_path).exists() {
            fs::write(&zip_path, zip_contents)?;
        } else {
            let existing_zip_contents = fs::read(&zip_path)?;
            if zip_contents != existing_zip_contents {
                fs::write(&zip_path, zip_contents)?;
            }
        }

        // Add zip bytes to bootstrapped_processes.rs
        writeln!(
            bootstrapped_processes,
            "    (\"{}\", include_bytes!(\"{}\")),",
            zip_filename, zip_path,
        )
        .unwrap();
    }
    writeln!(bootstrapped_processes, "];").unwrap();
    let bootstrapped_processes_path = pwd.join("src/bootstrapped_processes.rs");
    if bootstrapped_processes_path.exists() {
        let existing_bootstrapped_processes = fs::read(&bootstrapped_processes_path)?;
        if bootstrapped_processes == existing_bootstrapped_processes {
            return Ok(());
        }
    }
    fs::write(&bootstrapped_processes_path, bootstrapped_processes)?;
    Ok(())
}
