use dashmap::DashMap;
use lib::types::core::{
    Address, CapMessage, CapMessageSender, Capability, DirEntry, FdManagerRequest, FileMetadata,
    FileType, KernelMessage, LazyLoadBlob, Message, MessageReceiver, MessageSender, PackageId,
    PrintSender, Printout, ProcessId, Request, Response, VfsAction, VfsError, VfsRequest,
    VfsResponse, FD_MANAGER_PROCESS_ID, KERNEL_PROCESS_ID, VFS_PROCESS_ID,
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

    let mut files = Files::new(
        Address::new(our_node.as_str(), VFS_PROCESS_ID.clone()),
        send_to_loop,
    );

    let process_queues: HashMap<ProcessId, Arc<Mutex<VecDeque<KernelMessage>>>> =
        HashMap::default();

    crate::fd_manager::send_fd_manager_request_fds_limit(&files.our, &files.send_to_loop).await;

    while let Some(km) = recv_from_loop.recv().await {
        if *our_node != km.source.node {
            Printout::new(
                1,
                VFS_PROCESS_ID.clone(),
                format!(
                    "vfs: got request from {}, but requests must come from our node {our_node}",
                    km.source.node
                ),
            )
            .send(&send_to_terminal)
            .await;
            continue;
        }

        if km.source.process == *FD_MANAGER_PROCESS_ID {
            if let Err(e) = handle_fd_request(km, &mut files).await {
                Printout::new(
                    1,
                    VFS_PROCESS_ID.clone(),
                    format!("vfs: got request from fd-manager that errored: {e:?}"),
                )
                .send(&send_to_terminal)
                .await;
            };
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
        let send_to_caps_oracle = send_to_caps_oracle.clone();
        let mut files = files.clone();
        let vfs_path = vfs_path.clone();

        tokio::spawn(async move {
            let mut queue_lock = queue.lock().await;
            if let Some(km) = queue_lock.pop_front() {
                let (km_id, km_rsvp) =
                    (km.id.clone(), km.rsvp.clone().unwrap_or(km.source.clone()));

                let expects_response = match km.message {
                    Message::Request(Request {
                        expects_response, ..
                    }) => expects_response.is_some(),
                    _ => false,
                };

                if let Err(e) =
                    handle_request(&our_node, km, &mut files, &send_to_caps_oracle, &vfs_path).await
                {
                    if expects_response {
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
                            .send(&files.send_to_loop)
                            .await;
                    }
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
    pub our: Address,
    pub send_to_loop: MessageSender,
    pub fds_limit: u64,
}

struct FileEntry {
    file: Arc<Mutex<fs::File>>,
    last_access: Instant,
}

impl Files {
    pub fn new(our: Address, send_to_loop: MessageSender) -> Self {
        Self {
            open_files: Arc::new(DashMap::new()),
            cursor_positions: Arc::new(DashMap::new()),
            access_order: Arc::new(Mutex::new(UniqueQueue::new())),
            our,
            send_to_loop,
            fds_limit: 10, // small hardcoded limit that gets replaced by fd-manager soon after boot
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

        // if open files >= fds_limit, close the (limit/2) least recently used files
        if self.open_files.len() as u64 >= self.fds_limit {
            crate::fd_manager::send_fd_manager_hit_fds_limit(&self.our, &self.send_to_loop).await;
            self.close_least_recently_used_files(self.fds_limit / 2)
                .await?;
        }

        Ok(file)
    }

    async fn remove_file(&self, path: &Path) -> Result<(), VfsError> {
        self.open_files.remove(path);
        Ok(())
    }

    async fn update_access_order(&self, path: &Path) {
        let mut access_order = self.access_order.lock().await;
        access_order.push_back(path.to_path_buf());
    }

    async fn close_least_recently_used_files(&self, to_close: u64) -> Result<(), VfsError> {
        let mut access_order = self.access_order.lock().await;
        let mut closed = 0;

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
    files: &mut Files,
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
        // we got a response -- safe to ignore
        return Ok(());
    };

    let request: VfsRequest =
        serde_json::from_slice(&body).map_err(|_| VfsError::MalformedRequest)?;

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
                .send(&files.send_to_loop)
                .await;
            return Ok(());
        } else {
            return Err(VfsError::NoReadCap);
        }
    }

    // current prepend to filepaths needs to be: /package_id/drive/path
    let (package_id, drive, rest) = parse_package_and_drive(&request.path, &vfs_path)?;
    // must have prepended `/` here or else it messes up caps downstream, e.g. in run-tests
    let drive = format!("/{package_id}/{drive}");
    let action = request.action;

    if km.source.process != *KERNEL_PROCESS_ID {
        check_caps(
            our_node,
            &km.source,
            &send_to_caps_oracle,
            &action,
            &drive,
            &package_id,
            vfs_path,
        )
        .await?;
    }
    // real safe path that the vfs will use
    let base_drive = join_paths_safely(&vfs_path, &drive);
    let path = join_paths_safely(&base_drive, &rest);

    #[cfg(target_os = "windows")]
    let (path, internal_path) = (internal_path_to_external(&path), path);

    let (response_body, bytes) = match action {
        VfsAction::CreateDrive => {
            #[cfg(target_os = "windows")]
            let base_drive = internal_path_to_external(&base_drive);

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
            files.remove_file(&path).await?;
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
            files.remove_file(&path).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::WriteAll => {
            // doesn't create a file, writes at exact cursor.
            let Some(blob) = km.lazy_load_blob else {
                return Err(VfsError::NoBlob);
            };
            let file = files.open_file(&path, false, false).await?;
            let mut file = file.lock().await;
            file.write_all(&blob.bytes).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::Write => {
            let Some(blob) = km.lazy_load_blob else {
                return Err(VfsError::NoBlob);
            };
            fs::write(&path, &blob.bytes).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::Append => {
            let Some(blob) = km.lazy_load_blob else {
                return Err(VfsError::NoBlob);
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
        VfsAction::ReadExact { length } => {
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

                #[cfg(unix)]
                let relative_path = relative_path.display().to_string();
                #[cfg(target_os = "windows")]
                let relative_path = {
                    let internal_path = internal_path
                        .strip_prefix(vfs_path)
                        .unwrap_or(&internal_path);
                    replace_path_prefix(&internal_path, &relative_path)
                };

                let dir_entry = DirEntry {
                    path: relative_path,
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
        VfsAction::Seek(seek_from) => {
            let file = files.open_file(&path, false, false).await?;
            let mut file = file.lock().await;
            let seek_from = match seek_from {
                lib::types::core::SeekFrom::Start(offset) => std::io::SeekFrom::Start(offset),
                lib::types::core::SeekFrom::End(offset) => std::io::SeekFrom::End(offset),
                lib::types::core::SeekFrom::Current(offset) => std::io::SeekFrom::Current(offset),
            };
            let response = file.seek(seek_from).await?;
            (
                VfsResponse::SeekFrom {
                    new_offset: response,
                },
                None,
            )
        }
        VfsAction::RemoveFile => {
            fs::remove_file(&path).await?;
            files.remove_file(&path).await?;
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
            fs::rename(&path, new_path).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::CopyFile { new_path } => {
            let new_path = join_paths_safely(vfs_path, &new_path);
            fs::copy(&path, new_path).await?;
            (VfsResponse::Ok, None)
        }
        VfsAction::Metadata => {
            let metadata = fs::metadata(&path).await?;
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
            let len = file.metadata().await?.len();
            (VfsResponse::Len(len), None)
        }
        VfsAction::SetLen(len) => {
            let file = files.open_file(&path, false, false).await?;
            let file = file.lock().await;
            file.set_len(len).await?;
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
                return Err(VfsError::NoBlob);
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
                    let mut file = zip.by_index(i).map_err(|_| VfsError::UnzipError)?;
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
                    return Err(VfsError::UnzipError);
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
            .send(&files.send_to_loop)
            .await;
    }

    Ok(())
}

fn parse_package_and_drive(
    path: &str,
    vfs_path: &PathBuf,
) -> Result<(PackageId, String, PathBuf), VfsError> {
    let joined_path = join_paths_safely(&vfs_path, path);

    // sanitize path..
    let normalized_path = normalize_path(&joined_path);
    if !normalized_path.starts_with(vfs_path) {
        return Err(VfsError::MalformedRequest);
    }

    // extract original path.
    let path = normalized_path
        .strip_prefix(vfs_path)
        .map_err(|_| VfsError::MalformedRequest)?
        .display()
        .to_string();

    #[cfg(unix)]
    let mut parts: Vec<&str> = path.split('/').collect();
    #[cfg(target_os = "windows")]
    let mut parts: Vec<&str> = path.split('\\').collect();

    if parts[0].is_empty() {
        parts.remove(0);
    }
    if parts.len() < 2 {
        return Err(VfsError::MalformedRequest);
    }

    let package_id = match parts[0].parse::<PackageId>() {
        Ok(id) => id,
        Err(_) => {
            return Err(VfsError::MalformedRequest);
        }
    };

    let drive = parts[1].to_string();
    let mut remaining_path = PathBuf::new();
    for part in &parts[2..] {
        remaining_path = remaining_path.join(part);
    }

    Ok((package_id, drive, remaining_path))
}

#[cfg(target_os = "windows")]
fn internal_path_to_external(internal: &Path) -> PathBuf {
    let mut external = PathBuf::new();
    for component in internal.components() {
        match component {
            Component::RootDir | Component::CurDir | Component::ParentDir => {}
            Component::Prefix(_) => {
                let component: &Path = component.as_ref();
                external = component.to_path_buf();
            }
            Component::Normal(item) => {
                external = external.join(item.to_string_lossy().into_owned().replace(":", "_"));
            }
        }
    }

    external
}

#[cfg(target_os = "windows")]
fn replace_path_prefix(base_path: &Path, to_replace_path: &Path) -> String {
    let base_path = base_path.display().to_string();
    let base_path_parts: Vec<&str> = base_path.split('\\').collect();

    let num_base_path_parts = base_path_parts.len();

    let to_replace_path = to_replace_path.display().to_string();
    let parts: Vec<&str> = to_replace_path.split('\\').collect();

    let mut new_path = base_path.to_string().replace("\\", "/");
    for part in parts.iter().skip(num_base_path_parts) {
        new_path.push('/');
        new_path.push_str(part);
    }
    new_path
}

async fn check_caps(
    our_node: &str,
    source: &Address,
    send_to_caps_oracle: &CapMessageSender,
    action: &VfsAction,
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
                return Err(VfsError::NoWriteCap);
            }
            Ok(())
        }
        VfsAction::Read
        | VfsAction::ReadDir
        | VfsAction::ReadExact { .. }
        | VfsAction::ReadToEnd
        | VfsAction::ReadToString
        | VfsAction::Seek(_)
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
                return Err(VfsError::NoReadCap);
            }
            Ok(())
        }
        VfsAction::CopyFile { new_path } | VfsAction::Rename { new_path } => {
            // these have 2 paths to validate
            let (new_package_id, new_drive, _rest) = parse_package_and_drive(new_path, &vfs_path)?;

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
                return Err(VfsError::NoWriteCap);
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
                return Err(VfsError::NoWriteCap);
            }
            Ok(())
        }
        VfsAction::CreateDrive => {
            if &src_package_id != package_id {
                // check for root cap
                if !read_capability("", "", true, our_node, source, send_to_caps_oracle).await {
                    return Err(VfsError::NoWriteCap);
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
    let Ok(()) = send_to_caps_oracle
        .send(CapMessage::Add {
            on: source.process.clone(),
            caps: vec![cap],
            responder: Some(send_cap_bool),
        })
        .await
    else {
        return Err(VfsError::AddCapFailed);
    };
    let Ok(true) = recv_cap_bool.await else {
        return Err(VfsError::AddCapFailed);
    };
    Ok(())
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
        .trim_start_matches('/')
        .trim_start_matches('\\');

    let extension_path = Path::new(extension_str);
    base.join(extension_path)
}

async fn handle_fd_request(km: KernelMessage, files: &mut Files) -> anyhow::Result<()> {
    let Message::Request(Request { body, .. }) = km.message else {
        return Err(anyhow::anyhow!("not a request"));
    };

    let request: FdManagerRequest = serde_json::from_slice(&body)?;

    match request {
        FdManagerRequest::FdsLimit(fds_limit) => {
            files.fds_limit = fds_limit;
            if files.open_files.len() as u64 >= fds_limit {
                crate::fd_manager::send_fd_manager_hit_fds_limit(&files.our, &files.send_to_loop)
                    .await;
                files
                    .close_least_recently_used_files(files.open_files.len() as u64 - fds_limit)
                    .await?;
            }
        }
        _ => {
            return Err(anyhow::anyhow!("non-Cull FdManagerRequest"));
        }
    }

    Ok(())
}
