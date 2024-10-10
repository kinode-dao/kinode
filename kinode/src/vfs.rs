use dashmap::DashMap;
use lib::types::core::{
    Address, CapMessage, CapMessageSender, Capability, DirEntry, FileMetadata, FileType,
    KernelMessage, LazyLoadBlob, Message, MessageReceiver, MessageSender, PackageId, PrintSender,
    Printout, ProcessId, Request, Response, VfsAction, VfsError, VfsRequest, VfsResponse,
    KERNEL_PROCESS_ID, VFS_PROCESS_ID,
};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom},
    sync::Mutex,
};

// Constants for file cleanup
const MAX_OPEN_FILES: usize = 180;

/// The main VFS service function.
///
/// This function sets up the VFS, handles incoming requests, and manages file operations.
/// It also implements a file cleanup mechanism to close idle files.
///
/// # Arguments
/// * `our_node` - The identifier for the current node
/// * `send_to_loop` - Sender for kernel messages
/// * `send_to_terminal` - Sender for print messages
/// * `recv_from_loop` - Receiver for incoming messages
/// * `send_to_caps_oracle` - Sender for capability messages
/// * `home_directory_path` - Path to the home directory
///
/// # Returns
/// * `anyhow::Result<()>` - Should never return Ok, but will return fatal errors.
pub async fn vfs(
    our_node: Arc<String>,
    send_to_loop: MessageSender,
    send_to_terminal: PrintSender,
    mut recv_from_loop: MessageReceiver,
    send_to_caps_oracle: CapMessageSender,
    home_directory_path: PathBuf,
) -> anyhow::Result<()> {
    let vfs_path = home_directory_path.join("vfs");

    fs::create_dir_all(&vfs_path)
        .await
        .map_err(|e| anyhow::anyhow!("failed creating vfs dir! {e:?}"))?;
    let vfs_path = Arc::new(fs::canonicalize(&vfs_path).await?);

    let files = Files::new();

    let process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> =
        HashMap::default();

    while let Some(km) = recv_from_loop.recv().await {
        if *our_node != km.source.node {
            Printout::new(
                1,
                format!(
                    "vfs: got request from {}, but requests must come from our node {our_node}",
                    km.source.node
                ),
            )
            .send(&send_to_terminal)
            .await;
            continue;
        }

        let queue = process_queues
            .get(&km.source.process)
            .cloned()
            .unwrap_or_else(|| Arc::new(Mutex::new(VecDeque::new())));

        {
            let mut queue_lock = queue.lock().await;
            queue_lock.push_back(km);
        }

        // Clone Arcs for the new task
        let our_node = our_node.clone();
        let send_to_loop = send_to_loop.clone();
        let send_to_caps_oracle = send_to_caps_oracle.clone();
        let files = files.clone();
        let vfs_path = vfs_path.clone();

        tokio::spawn(async move {
            let mut queue_lock = queue.lock().await;
            if let Some(km) = queue_lock.pop_front() {
                let (km_id, km_rsvp) =
                    (km.id.clone(), km.rsvp.clone().unwrap_or(km.source.clone()));

                if let Err(e) = handle_request(
                    &our_node,
                    km,
                    files,
                    &send_to_loop,
                    &send_to_caps_oracle,
                    &vfs_path,
                )
                .await
                {
                    KernelMessage::builder()
                        .id(km_id)
                        .source((our_node.as_str(), VFS_PROCESS_ID.clone()))
                        .target(km_rsvp)
                        .message(Message::Response((
                            Response {
                                inherit: false,
                                body: serde_json::to_vec(&VfsResponse::Err(e)).unwrap(),
                                metadata: None,
                                capabilities: vec![],
                            },
                            None,
                        )))
                        .build()
                        .unwrap()
                        .send(&send_to_loop)
                        .await;
                }
            }
        });
    }
    Ok(())
}

/// Helper struct to manage open files, cursor positions, and access order.
#[derive(Clone)]
struct Files {
    /// currently open files, with last access time
    open_files: Arc<DashMap<PathBuf, FileEntry>>,
    /// cursor positions for files closed to avoid too many open files OS error
    cursor_positions: Arc<DashMap<PathBuf, u64>>,
    /// access order of files
    access_order: Arc<Mutex<UniqueQueue<PathBuf>>>,
}

struct FileEntry {
    file: Arc<Mutex<fs::File>>,
    last_access: Instant,
}

impl Files {
    pub fn new() -> Self {
        Self {
            open_files: Arc::new(DashMap::new()),
            cursor_positions: Arc::new(DashMap::new()),
            access_order: Arc::new(Mutex::new(UniqueQueue::new())),
        }
    }

    pub async fn open_file<P: AsRef<Path>>(
        &self,
        path: P,
        create: bool,
        truncate: bool,
    ) -> Result<Arc<Mutex<fs::File>>, VfsError> {
        let path = path.as_ref().to_path_buf();

        if let Some(mut entry) = self.open_files.get_mut(&path) {
            entry.value_mut().last_access = Instant::now();
            self.update_access_order(&path).await;
            return Ok(entry.value().file.clone());
        }

        if self.open_files.len() >= MAX_OPEN_FILES {
            self.close_least_recently_used_files().await?;
        }

        let mut file = self.try_open_file(&path, create, truncate).await?;
        if let Some(position) = self.cursor_positions.get(&path) {
            file.seek(SeekFrom::Start(*position)).await?;
        }
        let file = Arc::new(Mutex::new(file));
        self.open_files.insert(
            path.clone(),
            FileEntry {
                file: file.clone(),
                last_access: Instant::now(),
            },
        );
        self.update_access_order(&path).await;
        Ok(file)
    }

    async fn update_access_order(&self, path: &Path) {
        let mut access_order = self.access_order.lock().await;
        access_order.push_back(path.to_path_buf());
    }

    async fn close_least_recently_used_files(&self) -> Result<(), VfsError> {
        let mut access_order = self.access_order.lock().await;
        let mut closed = 0;
        let to_close = MAX_OPEN_FILES / 3; // close 33% of max open files

        while closed < to_close {
            if let Some(path) = access_order.pop_front() {
                if let Some((_, file_entry)) = self.open_files.remove(&path) {
                    if Arc::strong_count(&file_entry.file) == 1 {
                        let mut file = file_entry.file.lock().await;
                        if let Ok(position) = file.stream_position().await {
                            if position != 0 {
                                self.cursor_positions.insert(path, position);
                            }
                        }
                        closed += 1;
                    } else {
                        // file is still in use, put it back
                        self.open_files.insert(path.clone(), file_entry);
                        access_order.push_back(path);
                    }
                }
            } else {
                break; // no more files to close
            }
        }
        Ok(())
    }

    async fn try_open_file(
        &self,
        path: &Path,
        create: bool,
        truncate: bool,
    ) -> Result<fs::File, std::io::Error> {
        tokio::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .truncate(truncate)
            .open(path)
            .await
    }
}

/// Handles individual VFS requests.
///
/// This function processes various VFS actions such as file operations, directory listings, etc.
///
/// # Arguments
/// * `our_node` - The identifier for the current node
/// * `km` - The incoming kernel message
/// * `files` - A struct containing open_files, cursor_positions, and access_order
/// * `send_to_loop` - Sender for kernel messages
/// * `send_to_caps_oracle` - Sender for capability messages
/// * `vfs_path` - The base path for the VFS
///
/// # Returns
/// * `Result<(), VfsError>` - Result indicating success or a VFS-specific error
async fn handle_request(
    our_node: &str,
    km: KernelMessage,
    files: Files,
    send_to_loop: &MessageSender,
    send_to_caps_oracle: &CapMessageSender,
    vfs_path: &PathBuf,
) -> Result<(), VfsError> {
    let Message::Request(Request {
        body,
        expects_response,
        metadata,
        ..
    }) = km.message
    else {
        return Err(VfsError::BadRequest {
            error: "not a request".into(),
        });
    };

    let request: VfsRequest = serde_json::from_slice(&body).map_err(|e| VfsError::BadJson {
        error: e.to_string(),
    })?;

    // special case for root reading list of all drives.
    if request.action == VfsAction::ReadDir && request.path == "/" {
        // check if src has root
        let has_root_cap =
            read_capability("", "", true, our_node, &km.source, send_to_caps_oracle).await;
        if has_root_cap {
            let mut dir = fs::read_dir(&vfs_path).await?;
            let mut entries = Vec::new();
            while let Some(entry) = dir.next_entry().await? {
                let entry_path = entry.path();
                let relative_path = entry_path.strip_prefix(&vfs_path).unwrap_or(&entry_path);

                let metadata = entry.metadata().await?;
                let file_type = get_file_type(&metadata);
                let dir_entry = DirEntry {
                    path: relative_path.display().to_string(),
                    file_type,
                };
                entries.push(dir_entry);
            }

            KernelMessage::builder()
                .id(km.id)
                .source((our_node, VFS_PROCESS_ID.clone()))
                .target(km.source)
                .message(Message::Response((
                    Response {
                        inherit: false,
                        body: serde_json::to_vec(&VfsResponse::ReadDir(entries)).unwrap(),
                        metadata,
                        capabilities: vec![],
                    },
                    None,
                )))
                .build()
                .unwrap()
                .send(send_to_loop)
                .await;
            return Ok(());
        } else {
            return Err(VfsError::NoCap {
                action: request.action.to_string(),
                path: request.path,
            });
        }
    }

    // current prepend to filepaths needs to be: /package_id/drive/path
    let (package_id, drive, rest) = parse_package_and_drive(&request.path, &vfs_path).await?;
    let drive = format!("{package_id}/{drive}");
    let action = request.action;
    let path = PathBuf::from(&request.path);

    if km.source.process != *KERNEL_PROCESS_ID {
        check_caps(
            our_node,
            &km.source,
            &send_to_caps_oracle,
            &action,
            &path,
            &drive,
            &package_id,
            vfs_path,
        )
        .await?;
    }
    // real safe path that the vfs will use
    let base_drive = join_paths_safely(&vfs_path, &drive);
    let path = join_paths_safely(&base_drive, &rest);

    let (response_body, bytes) = match action {
        VfsAction::CreateDrive => {
            fs::create_dir_all(&base_drive).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::CreateDir => {
            fs::create_dir(&path).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::CreateDirAll => {
            fs::create_dir_all(&path).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::CreateFile => {
            // create truncates any file that might've existed before
            files.open_files.remove(&path);
            let _file = files.open_file(&path, true, true).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::OpenFile { create } => {
            let file = files.open_file(&path, create, false).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::Start(0)).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::CloseFile => {
            // removes file from scope, resets file_handle and cursor.
            files.open_files.remove(&path);
            (VfsResponse::Ok, None)
        }
        VfsAction::WriteAll => {
            // doesn't create a file, writes at exact cursor.
            let Some(blob) = km.lazy_load_blob else {
                return Err(VfsError::BadRequest {
                    error: "blob needs to exist for WriteAll".into(),
                });
            };
            let file = files.open_file(&path, false, false).await?;
            let mut file = file.lock().await;
            file.write_all(&blob.bytes).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::Write => {
            let Some(blob) = km.lazy_load_blob else {
                return Err(VfsError::BadRequest {
                    error: "blob needs to exist for Write".into(),
                });
            };
            fs::write(&path, &blob.bytes).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::Append => {
            let Some(blob) = km.lazy_load_blob else {
                return Err(VfsError::BadRequest {
                    error: "blob needs to exist for Append".into(),
                });
            };
            let file = files.open_file(&path, false, false).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::End(0)).await?;
            file.write_all(&blob.bytes).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::SyncAll => {
            let file = files.open_file(&path, false, false).await?;
            let file = file.lock().await;
            file.sync_all().await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::Read => {
            let contents = fs::read(&path).await?;
            (VfsResponse::Read, Some(contents))
        }
        VfsAction::ReadToEnd => {
            let file = files.open_file(&path, false, false).await?;
            let mut file = file.lock().await;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents).await?;
            (VfsResponse::Read, Some(contents))
        }
        VfsAction::ReadExact(length) => {
            let file = files.open_file(&path, false, false).await?;
            let mut file = file.lock().await;
            let mut contents = vec![0; length as usize];
            file.read_exact(&mut contents).await?;
            (VfsResponse::Read, Some(contents))
        }
        VfsAction::ReadDir => {
            let mut dir = fs::read_dir(&path).await?;
            let mut entries = Vec::new();
            while let Some(entry) = dir.next_entry().await? {
                let entry_path = entry.path();
                let relative_path = entry_path.strip_prefix(vfs_path).unwrap_or(&entry_path);

                let metadata = entry.metadata().await?;
                let file_type = get_file_type(&metadata);
                let dir_entry = DirEntry {
                    path: relative_path.display().to_string(),
                    file_type,
                };
                entries.push(dir_entry);
            }
            (VfsResponse::ReadDir(entries), None)
        }
        VfsAction::ReadToString => {
            let file = files.open_file(&path, false, false).await?;
            let mut file = file.lock().await;
            let mut contents = String::new();
            file.read_to_string(&mut contents).await?;
            (VfsResponse::ReadToString(contents), None)
        }
        VfsAction::Seek { seek_from } => {
            let file = files.open_file(&path, false, false).await?;
            let mut file = file.lock().await;
            let seek_from = match seek_from {
                lib::types::core::SeekFrom::Start(offset) => std::io::SeekFrom::Start(offset),
                lib::types::core::SeekFrom::End(offset) => std::io::SeekFrom::End(offset),
                lib::types::core::SeekFrom::Current(offset) => std::io::SeekFrom::Current(offset),
            };
            let response = file.seek(seek_from).await?;
            (VfsResponse::SeekFrom(response), None)
        }
        VfsAction::RemoveFile => {
            fs::remove_file(&path).await?;
            files.open_files.remove(&path);
            (VfsResponse::Ok, None)
        }
        VfsAction::RemoveDir => {
            fs::remove_dir(&path).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::RemoveDirAll => {
            fs::remove_dir_all(&path).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::Rename { new_path } => {
            let new_path = join_paths_safely(vfs_path, &new_path);
            fs::rename(&path, new_path)
                .await
                .map_err(|e| VfsError::IOError {
                    error: e.to_string(),
                    path: request.path,
                })?;
            (VfsResponse::Ok, None)
        }
        VfsAction::CopyFile { new_path } => {
            let new_path = join_paths_safely(vfs_path, &new_path);
            fs::copy(&path, new_path)
                .await
                .map_err(|e| VfsError::IOError {
                    error: e.to_string(),
                    path: request.path,
                })?;
            (VfsResponse::Ok, None)
        }
        VfsAction::Metadata => {
            let metadata = fs::metadata(&path).await.map_err(|e| VfsError::IOError {
                error: e.to_string(),
                path: request.path,
            })?;
            let file_type = get_file_type(&metadata);
            let meta = FileMetadata {
                len: metadata.len(),
                file_type,
            };
            (VfsResponse::Metadata(meta), None)
        }
        VfsAction::Len => {
            let file = files.open_file(&path, false, false).await?;
            let file = file.lock().await;
            let len = file
                .metadata()
                .await
                .map_err(|e| VfsError::IOError {
                    error: e.to_string(),
                    path: request.path,
                })?
                .len();
            (VfsResponse::Len(len), None)
        }
        VfsAction::SetLen(len) => {
            let file = files.open_file(&path, false, false).await?;
            let file = file.lock().await;
            file.set_len(len).await.map_err(|e| VfsError::IOError {
                error: e.to_string(),
                path: request.path,
            })?;
            (VfsResponse::Ok, None)
        }
        VfsAction::Hash => {
            use sha2::{Digest, Sha256};
            let file = files.open_file(&path, false, false).await?;
            let mut file = file.lock().await;
            file.seek(SeekFrom::Start(0)).await?;
            let mut hasher = Sha256::new();
            let mut buffer = [0; 1024];
            loop {
                let bytes_read = file.read(&mut buffer).await?;
                if bytes_read == 0 {
                    break;
                }
                hasher.update(&buffer[..bytes_read]);
            }
            let hash: [u8; 32] = hasher.finalize().into();
            (VfsResponse::Hash(hash), None)
        }
        VfsAction::AddZip => {
            let Some(blob) = km.lazy_load_blob else {
                return Err(VfsError::BadRequest {
                    error: "blob needs to exist for AddZip".into(),
                });
            };
            let file = std::io::Cursor::new(&blob.bytes);
            let mut zip = match zip::ZipArchive::new(file) {
                Ok(f) => f,
                Err(e) => {
                    return Err(VfsError::ParseError {
                        error: e.to_string(),
                        path: path.display().to_string(),
                    })
                }
            };

            fs::create_dir_all(&path).await?;

            // loop through items in archive; recursively add to root
            for i in 0..zip.len() {
                // must destruct the zip file created in zip.by_index()
                //  Before any `.await`s are called since ZipFile is not
                //  Send and so does not play nicely with await
                let (is_file, is_dir, local_path, file_contents) = {
                    let mut file = zip.by_index(i).map_err(|e| VfsError::IOError {
                        error: e.to_string(),
                        path: request.path.clone(),
                    })?;
                    let is_file = file.is_file();
                    let is_dir = file.is_dir();
                    let mut file_contents = Vec::new();
                    if is_file {
                        file.read_to_end(&mut file_contents)?;
                    };
                    let local_path = path.join(file.name());
                    (is_file, is_dir, local_path, file_contents)
                };
                if is_file {
                    fs::write(&local_path, &file_contents).await?;
                } else if is_dir {
                    fs::create_dir_all(&local_path).await?;
                } else {
                    return Err(VfsError::CreateDirError {
                        path: path.display().to_string(),
                        error: "vfs: zip with non-file non-dir".into(),
                    });
                };
            }
            (VfsResponse::Ok, None)
        }
    };

    if let Some(target) = km.rsvp.or_else(|| expects_response.map(|_| km.source)) {
        KernelMessage::builder()
            .id(km.id)
            .source((our_node, VFS_PROCESS_ID.clone()))
            .target(target)
            .message(Message::Response((
                Response {
                    inherit: false,
                    body: serde_json::to_vec(&response_body).unwrap(),
                    metadata,
                    capabilities: vec![],
                },
                None,
            )))
            .lazy_load_blob(bytes.map(|bytes| LazyLoadBlob {
                mime: Some("application/octet-stream".into()),
                bytes,
            }))
            .build()
            .unwrap()
            .send(send_to_loop)
            .await;
    }

    Ok(())
}

async fn parse_package_and_drive(
    path: &str,
    vfs_path: &PathBuf,
) -> Result<(PackageId, String, PathBuf), VfsError> {
    let joined_path = join_paths_safely(&vfs_path, path);

    // sanitize path..
    let normalized_path = normalize_path(&joined_path);
    if !normalized_path.starts_with(vfs_path) {
        return Err(VfsError::BadRequest {
            error: format!("input path tries to escape parent vfs directory: {path}"),
        })?;
    }

    // extract original path.
    let path = normalized_path
        .strip_prefix(vfs_path)
        .map_err(|_| VfsError::BadRequest {
            error: format!("input path tries to escape parent vfs directory: {path}"),
        })?
        .display()
        .to_string();

    let mut parts: Vec<&str> = path.split('/').collect();

    if parts[0].is_empty() {
        parts.remove(0);
    }
    if parts.len() < 2 {
        return Err(VfsError::ParseError {
            error: "malformed path".into(),
            path,
        });
    }

    let package_id = match parts[0].parse::<PackageId>() {
        Ok(id) => id,
        Err(e) => {
            return Err(VfsError::ParseError {
                error: e.to_string(),
                path,
            })
        }
    };

    let drive = parts[1].to_string();
    let mut remaining_path = PathBuf::new();
    for part in &parts[2..] {
        remaining_path = remaining_path.join(part);
    }

    Ok((package_id, drive, remaining_path))
}

async fn check_caps(
    our_node: &str,
    source: &Address,
    send_to_caps_oracle: &CapMessageSender,
    action: &VfsAction,
    path: &PathBuf,
    drive: &str,
    package_id: &PackageId,
    vfs_path: &PathBuf,
) -> Result<(), VfsError> {
    let src_package_id = PackageId::new(source.process.package(), source.process.publisher());

    // every action is valid if package has vfs root cap, but this should only be
    // checked for *after* non-root caps are checked, because 99% of the time,
    // package will have regular read/write cap regardless of root status.
    match &action {
        VfsAction::CreateDir
        | VfsAction::CreateDirAll
        | VfsAction::CreateFile
        | VfsAction::OpenFile { .. }
        | VfsAction::CloseFile
        | VfsAction::Write
        | VfsAction::WriteAll
        | VfsAction::Append
        | VfsAction::SyncAll
        | VfsAction::RemoveFile
        | VfsAction::RemoveDir
        | VfsAction::RemoveDirAll
        | VfsAction::AddZip
        | VfsAction::SetLen(_) => {
            if &src_package_id == package_id {
                return Ok(());
            }
            let has_cap =
                read_capability("write", drive, false, our_node, source, send_to_caps_oracle).await;
            if !has_cap {
                // check for root cap
                if read_capability("", "", true, our_node, source, send_to_caps_oracle).await {
                    return Ok(());
                }
                return Err(VfsError::NoCap {
                    action: action.to_string(),
                    path: path.display().to_string(),
                });
            }
            Ok(())
        }
        VfsAction::Read
        | VfsAction::ReadDir
        | VfsAction::ReadExact(_)
        | VfsAction::ReadToEnd
        | VfsAction::ReadToString
        | VfsAction::Seek { .. }
        | VfsAction::Hash
        | VfsAction::Metadata
        | VfsAction::Len => {
            if &src_package_id == package_id {
                return Ok(());
            }
            let has_cap =
                read_capability("read", drive, false, our_node, source, send_to_caps_oracle).await;
            if !has_cap {
                // check for root cap
                if read_capability("", "", true, our_node, source, send_to_caps_oracle).await {
                    return Ok(());
                }
                return Err(VfsError::NoCap {
                    action: action.to_string(),
                    path: path.display().to_string(),
                });
            }
            Ok(())
        }
        VfsAction::CopyFile { new_path } | VfsAction::Rename { new_path } => {
            // these have 2 paths to validate
            let (new_package_id, new_drive, _rest) =
                parse_package_and_drive(new_path, &vfs_path).await?;

            let new_drive = format!("/{new_package_id}/{new_drive}");
            // if both new and old path are within the package_id path, ok
            if (&src_package_id == package_id) && (src_package_id == new_package_id) {
                return Ok(());
            }

            // otherwise check write caps.
            let has_cap = read_capability(
                "write",
                &drive,
                false,
                our_node,
                source,
                send_to_caps_oracle,
            )
            .await;
            if !has_cap {
                // check for root cap
                if read_capability("", "", true, our_node, source, send_to_caps_oracle).await {
                    return Ok(());
                }
                return Err(VfsError::NoCap {
                    action: action.to_string(),
                    path: path.display().to_string(),
                });
            }

            // if they're within the same drive, no need for 2 caps checks
            if new_drive == drive {
                return Ok(());
            }

            let has_cap = read_capability(
                "write",
                &new_drive,
                false,
                our_node,
                source,
                send_to_caps_oracle,
            )
            .await;
            if !has_cap {
                // check for root cap
                if read_capability("", "", true, our_node, source, send_to_caps_oracle).await {
                    return Ok(());
                }
                return Err(VfsError::NoCap {
                    action: action.to_string(),
                    path: path.display().to_string(),
                });
            }
            Ok(())
        }
        VfsAction::CreateDrive => {
            if &src_package_id != package_id {
                // check for root cap
                if !read_capability("", "", true, our_node, source, send_to_caps_oracle).await {
                    return Err(VfsError::NoCap {
                        action: action.to_string(),
                        path: path.display().to_string(),
                    });
                }
            }
            add_capability("read", &drive, &our_node, &source, send_to_caps_oracle).await?;
            add_capability("write", &drive, &our_node, &source, send_to_caps_oracle).await?;
            Ok(())
        }
    }
}

async fn read_capability(
    kind: &str,
    drive: &str,
    root: bool,
    our_node: &str,
    source: &Address,
    send_to_caps_oracle: &CapMessageSender,
) -> bool {
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    let cap = Capability::new(
        (our_node, VFS_PROCESS_ID.clone()),
        if root {
            "{\"root\":true}".to_string()
        } else {
            format!("{{\"kind\": \"{kind}\", \"drive\": \"{drive}\"}}")
        },
    );
    if let Err(_) = send_to_caps_oracle
        .send(CapMessage::Has {
            on: source.process.clone(),
            cap,
            responder: send_cap_bool,
        })
        .await
    {
        return false;
    }
    recv_cap_bool.await.unwrap_or(false)
}

async fn add_capability(
    kind: &str,
    drive: &str,
    our_node: &str,
    source: &Address,
    send_to_caps_oracle: &CapMessageSender,
) -> Result<(), VfsError> {
    let cap = Capability::new(
        (our_node, VFS_PROCESS_ID.clone()),
        format!("{{\"kind\": \"{kind}\", \"drive\": \"{drive}\"}}"),
    );
    let (send_cap_bool, recv_cap_bool) = tokio::sync::oneshot::channel();
    send_to_caps_oracle
        .send(CapMessage::Add {
            on: source.process.clone(),
            caps: vec![cap],
            responder: Some(send_cap_bool),
        })
        .await?;
    match recv_cap_bool.await? {
        true => Ok(()),
        false => Err(VfsError::NoCap {
            action: "add_capability".to_string(),
            path: drive.to_string(),
        }),
    }
}

fn get_file_type(metadata: &std::fs::Metadata) -> FileType {
    if metadata.is_file() {
        FileType::File
    } else if metadata.is_dir() {
        FileType::Directory
    } else if metadata.file_type().is_symlink() {
        FileType::Symlink
    } else {
        FileType::Other
    }
}

/// helper cache for most recently used paths

pub struct UniqueQueue<T>
where
    T: Eq + Hash,
{
    pub queue: VecDeque<T>,
    pub set: HashSet<T>,
}

#[allow(unused)]
impl<T> UniqueQueue<T>
where
    T: Eq + Hash + Clone,
{
    pub fn new() -> Self {
        UniqueQueue {
            queue: VecDeque::new(),
            set: HashSet::new(),
        }
    }

    pub fn push_back(&mut self, value: T) -> bool {
        if self.set.insert(value.clone()) {
            self.queue.push_back(value);
            true
        } else {
            false
        }
    }

    pub fn pop_front(&mut self) -> Option<T> {
        if let Some(value) = self.queue.pop_front() {
            self.set.remove(&value);
            Some(value)
        } else {
            None
        }
    }

    pub fn contains(&self, value: &T) -> bool {
        self.set.contains(value)
    }

    pub fn remove(&mut self, value: &T) -> bool {
        if self.set.remove(value) {
            self.queue.retain(|x| x != value);
            true
        } else {
            false
        }
    }
}

/// from rust/cargo/src/cargo/util/paths.rs
/// to avoid using std::fs::canonicalize, which fails on non-existent paths.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                ret.pop();
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}

fn join_paths_safely<P: AsRef<Path>>(base: &PathBuf, extension: P) -> PathBuf {
    let extension_str = extension
        .as_ref()
        .to_str()
        .unwrap_or("")
        .trim_start_matches('/');

    let extension_path = Path::new(extension_str);
    base.join(extension_path)
}
