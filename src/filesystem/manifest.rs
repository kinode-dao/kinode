use crate::types::*;
use crate::types::{FsError, ProcessId};
use blake3::Hasher;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use lru_mem::LruCache;
use rand::RngCore;
use rusoto_core::{Region, RusotoError};
use rusoto_s3::{
    DeleteObjectError, GetObjectError, GetObjectRequest, ListObjectsV2Error, PutObjectError,
    PutObjectRequest, S3Client, StreamingBody, S3,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::{self, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::RwLock;

/// Contains interface for filesystem manifest log, and write ahead log.

//   ON-DISK, WAL
#[derive(Serialize, Deserialize, Debug)]
pub enum WALRecord {
    CommitTx(u64),
    Chunk(ChunkEntry),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChunkEntry {
    file: FileIdentifier,
    tx_id: u64,
    start: u64,
    length: u64,
    chunk_hash: [u8; 32],
    copy_location: Option<ChunkLocation>,
    encrypted: bool,
    //  data: Vec<u8> is after this entry, if copy_location is None
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, Hash, PartialEq)]
pub enum FileIdentifier {
    Uuid(u128),
    Process(ProcessId),
}

//   ON-DISK, MANIFEST
#[derive(Serialize, Deserialize, Clone)]
pub enum ManifestRecord {
    Backup(BackupEntry),
    Delete(FileIdentifier),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BackupEntry {
    pub file: FileIdentifier,
    pub chunks: Vec<([u8; 32], u64, u64, bool, bool)>, // (hash, start, length, encrypted, local)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ChunkLocation {
    ColdStorage(bool), // bool local
    Wal(u64),          // offset in wal,
    Memory(u64),       // offset in memory buffer
}

// IN-MEMORY, MANIFEST
#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    pub start: u64,
    pub hash: [u8; 32],
    pub length: u64,
    pub location: ChunkLocation,
    pub encrypted: bool,
    pub mem_buffer_meta: Option<MemBufferMeta>,
}

#[derive(Debug, Clone, Copy)]
pub struct MemBufferMeta {
    pub copy: bool,
    pub tx_id: u64,
}

#[derive(Debug, Clone)]
pub enum MemChunkIndex {
    Chunk(u64),    // u64 == key into memory buffer
    CommitTx(u64), // u64 == tx_id
}

const NONCE_SIZE: usize = 24;
const TAG_SIZE: usize = 16;
const ENCRYPTION_OVERHEAD: usize = NONCE_SIZE + TAG_SIZE;

#[derive(Debug, Clone, Default)]
pub struct InMemoryFile {
    //  chunks: (start) -> chunk [commited txs]
    pub chunks: BTreeMap<u64, Chunk>,

    //  ongoing txs: (tx_id) -> chunk
    pub active_txs: HashMap<u64, Vec<Chunk>>,

    //  indexes for efficient flush (start: u64)
    pub mem_chunks: Vec<MemChunkIndex>,
    pub wal_chunks: Vec<u64>,
}

impl InMemoryFile {
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = Hasher::new();
        for chunk in self.chunks.values() {
            hasher.update(&chunk.hash);
        }
        hasher.finalize().into()
    }

    pub fn find_chunks_in_range(&self, start: u64, length: u64) -> Vec<Chunk> {
        let end = start + length;
        self.chunks
            .values()
            .filter(|chunk| {
                let chunk_length = if chunk.encrypted {
                    chunk.length + ENCRYPTION_OVERHEAD as u64
                } else {
                    chunk.length
                };
                let chunk_end = chunk.start + chunk_length;
                chunk.start < end && chunk_end > start
            })
            .cloned()
            .collect::<Vec<_>>()
    }

    pub fn get_len(&self) -> u64 {
        self.chunks
            .values()
            .last()
            .map_or(0, |chunk| chunk.start + chunk.length)
    }

    pub fn _get_last_chunk(&self) -> Option<Chunk> {
        self.chunks.values().last().cloned()
    }
}

impl FileIdentifier {
    pub fn new_uuid() -> Self {
        Self::Uuid(uuid::Uuid::new_v4().as_u128())
    }

    pub fn to_uuid(&self) -> Option<u128> {
        match self {
            Self::Uuid(uuid) => Some(*uuid),
            _ => None,
        }
    }
}
pub struct Manifest {
    pub manifest: Arc<RwLock<HashMap<FileIdentifier, InMemoryFile>>>,
    pub chunk_hashes: Arc<RwLock<HashMap<[u8; 32], ChunkLocation>>>,
    pub hash_index: Arc<RwLock<HashMap<[u8; 32], FileIdentifier>>>,

    pub manifest_file: Arc<RwLock<fs::File>>,
    pub wal_file: Arc<RwLock<fs::File>>,
    pub fs_directory_path: PathBuf,
    //  pub flush_wal_freq: usize,
    pub flush_cold_freq: usize,

    pub memory_buffer: Arc<RwLock<HashMap<u64, Vec<u8>>>>,
    pub membuf_size: Arc<RwLock<usize>>,
    pub read_cache: Arc<RwLock<LruCache<[u8; 32], Vec<u8>>>>,
    pub memory_limit: usize,

    pub chunk_size: usize,
    pub cipher: Arc<XChaCha20Poly1305>,
    pub encryption: bool,
    pub cloud_enabled: bool,
    pub s3_client: Option<(S3Client, String)>,
}

impl Manifest {
    pub async fn load(
        manifest_file: fs::File,
        wal_file: fs::File,
        fs_directory_path: &Path,
        file_key: Vec<u8>,
        fs_config: FsConfig,
    ) -> io::Result<Self> {
        let mut manifest = HashMap::new();
        let mut chunk_hashes = HashMap::new();
        let mut hash_index = HashMap::new();
        let mut manifest_file = manifest_file;
        let mut wal_file = wal_file;

        load_manifest(&mut manifest_file, &mut manifest).await?;
        load_wal(&mut wal_file, &mut manifest).await?;

        verify_manifest(&mut manifest, &mut chunk_hashes, &mut hash_index).await?;
        let cipher = XChaCha20Poly1305::new_from_slice(&file_key).unwrap();

        let read_cache: LruCache<[u8; 32], Vec<u8>> = LruCache::new(fs_config.read_cache_limit);

        let s3_client = if let Some(s3_config) = fs_config.s3_config {
            match parse_s3_config(s3_config) {
                Ok((s3_client, bucket)) => Some((s3_client, bucket)),
                Err(e) => {
                    println!("Failed to parse S3 config: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            manifest: Arc::new(RwLock::new(manifest)),
            chunk_hashes: Arc::new(RwLock::new(chunk_hashes)),
            hash_index: Arc::new(RwLock::new(hash_index)),
            manifest_file: Arc::new(RwLock::new(manifest_file)),
            wal_file: Arc::new(RwLock::new(wal_file)),
            fs_directory_path: fs_directory_path.to_path_buf(),
            flush_cold_freq: fs_config.flush_to_cold_interval,
            memory_buffer: Arc::new(RwLock::new(HashMap::new())),
            membuf_size: Arc::new(RwLock::new(0)),
            read_cache: Arc::new(RwLock::new(read_cache)),
            memory_limit: fs_config.mem_buffer_limit,
            chunk_size: fs_config.chunk_size,
            cipher: Arc::new(cipher),
            encryption: fs_config.encryption,
            cloud_enabled: fs_config.cloud_enabled,
            s3_client,
        })
    }

    pub async fn get(&self, file: &FileIdentifier) -> Option<InMemoryFile> {
        let read_lock = self.manifest.read().await;
        read_lock.get(file).cloned()
    }

    pub async fn get_length(&self, file: &FileIdentifier) -> Option<u64> {
        let read_lock = self.manifest.read().await;
        read_lock.get(file).map(|f| f.get_len())
    }

    pub async fn _get_memory_buffer_size(&self) -> usize {
        let read_lock = self.memory_buffer.read().await;
        read_lock.len()
    }

    pub async fn _get_total_bytes(&self) -> u64 {
        let read_lock = self.manifest.read().await;
        read_lock.values().fold(0, |acc, file| {
            acc + file.chunks.values().map(|chunk| chunk.length).sum::<u64>()
        })
    }

    pub async fn get_by_hash(&self, hash: &[u8; 32]) -> Option<FileIdentifier> {
        let read_lock = self.hash_index.read().await;
        read_lock.get(hash).cloned()
    }

    pub async fn _get_chunk_hashes(&self) -> HashSet<[u8; 32]> {
        let mut in_use_hashes = HashSet::new();
        for file in self.manifest.read().await.values() {
            for chunk in file.chunks.values() {
                in_use_hashes.insert(chunk.hash);
            }
        }
        in_use_hashes
    }

    pub async fn _get_file_hashes(&self) -> HashMap<FileIdentifier, [u8; 32]> {
        let mut file_hashes = HashMap::new();
        for (file_id, file) in self.manifest.read().await.iter() {
            file_hashes.insert(file_id.clone(), file.hash());
        }
        file_hashes
    }

    pub async fn _get_uuid_by_hash(&self, hash: &[u8; 32]) -> Option<u128> {
        let read_lock = self.hash_index.read().await;
        if let Some(file_id) = read_lock.get(hash) {
            file_id.to_uuid()
        } else {
            None
        }
    }

    pub async fn commit_tx(
        &self,
        tx_id: u64,
        in_memory_file: &mut InMemoryFile,
        memory_buffer: &mut HashMap<u64, Vec<u8>>,
        membuf_size: &mut usize,
    ) {
        let mut chunk_hashes = self.chunk_hashes.write().await;
        let commit_index = MemChunkIndex::CommitTx(tx_id);

        if let Some(tx_chunks) = in_memory_file.active_txs.remove(&tx_id) {
            for chunk in tx_chunks {
                if let Some(old_chunk) = in_memory_file.chunks.insert(chunk.start, chunk) {
                    if let ChunkLocation::Memory(old_mem_key) = old_chunk.location {
                        memory_buffer.remove(&old_mem_key);
                        *membuf_size -= old_chunk.length as usize;
                    }
                }
                match &chunk.location {
                    ChunkLocation::Memory(_idx) => {
                        in_memory_file
                            .mem_chunks
                            .push(MemChunkIndex::Chunk(chunk.start));
                    }
                    ChunkLocation::Wal(..) => {
                        in_memory_file.wal_chunks.push(chunk.start);
                        chunk_hashes.insert(chunk.hash, chunk.location);
                    }
                    _ => {}
                }
            }
            in_memory_file.mem_chunks.push(commit_index);
        }
    }

    pub async fn flush_to_wal(
        &self,
        manifest: &mut HashMap<FileIdentifier, InMemoryFile>,
        memory_buffer: &mut HashMap<u64, Vec<u8>>,
        membuf_size: &mut usize,
    ) -> Result<(), FsError> {
        let mut wal_file = self.wal_file.write().await;
        let wal_length_before_flush = wal_file.seek(SeekFrom::End(0)).await?;

        let mut wal_buffer: Vec<u8> = Vec::new();
        // let mut new_commited_locations: HashMap<FileIdentifier, (u64, ChunkLocation)> = HashMap::new();
        // let mut new_active_locations: HashMap<FileIdentifier, (u64, ChunkLocation)> = HashMap::new();

        for (file_id, in_memory_file) in manifest.iter_mut() {
            for index in &in_memory_file.mem_chunks {
                match index {
                    MemChunkIndex::Chunk(start) => {
                        if let Some(chunk) = in_memory_file.chunks.get_mut(&start) {
                            if let ChunkLocation::Memory(memkey) = chunk.location {
                                // serialize the chunk and write it to the buffer
                                match memory_buffer.get(&memkey) {
                                    None => {
                                        // should never happen
                                        continue;
                                    }
                                    Some(data) => {
                                        let serialized_chunk = serialize_chunk(
                                            file_id,
                                            data,
                                            chunk.start,
                                            chunk.mem_buffer_meta.unwrap().tx_id,
                                            chunk.encrypted,
                                            None,
                                            &self.cipher,
                                        )
                                        .await?;
                                        chunk.location = ChunkLocation::Wal(
                                            wal_length_before_flush + wal_buffer.len() as u64,
                                        );
                                        in_memory_file.wal_chunks.push(chunk.start);
                                        wal_buffer.extend_from_slice(&serialized_chunk);
                                    }
                                }
                            }
                        }
                    }
                    MemChunkIndex::CommitTx(tx_id) => {
                        let commit_tx = serialize_commit(*tx_id).await?;
                        wal_buffer.extend_from_slice(&commit_tx);
                    }
                }
            }

            // incomplete txs remain in their place, but location is in wal now
            for tx_chunks in in_memory_file.active_txs.values_mut() {
                for chunk in tx_chunks {
                    if let ChunkLocation::Memory(memkey) = &chunk.location {
                        // Serialize the chunk and write it to the buffer
                        let data = memory_buffer.get(memkey).unwrap();
                        let serialized_chunk = serialize_chunk(
                            file_id,
                            data,
                            chunk.start,
                            chunk.mem_buffer_meta.unwrap().tx_id,
                            chunk.encrypted,
                            None,
                            &self.cipher,
                        )
                        .await?;
                        chunk.location =
                            ChunkLocation::Wal(wal_length_before_flush + wal_buffer.len() as u64);
                        in_memory_file.wal_chunks.push(chunk.start);
                        wal_buffer.extend_from_slice(&serialized_chunk);
                    }
                }
            }
            in_memory_file.mem_chunks.clear();
        }

        wal_file.write_all(&wal_buffer).await?;
        wal_file.sync_all().await?;
        memory_buffer.clear();
        *membuf_size = 0;

        Ok(())
    }

    pub async fn flush_to_wal_main(&self) -> Result<(), FsError> {
        let mut manifest = self.manifest.write().await;

        let mut memory_buffer = self.memory_buffer.write().await;
        let mut membuf_size = self.membuf_size.write().await;
        self.flush_to_wal(&mut manifest, &mut memory_buffer, &mut membuf_size)
            .await?;
        Ok(())
    }

    pub async fn write(&self, file: &FileIdentifier, data: &[u8]) -> Result<(), FsError> {
        let mut manifest = self.manifest.write().await;
        let mut in_memory_file = InMemoryFile::default();
        let mut memory_buffer = self.memory_buffer.write().await;
        let mut membuf_size = self.membuf_size.write().await;

        let cipher: Option<&XChaCha20Poly1305> = if self.encryption {
            Some(&self.cipher)
        } else {
            None
        };

        let chunks = data.chunks(self.chunk_size);
        let mut chunk_start = 0u64;

        let tx_id = rand::random::<u64>(); // uuid instead?

        for chunk in chunks {
            if *membuf_size + chunk.len() > self.memory_limit {
                manifest.insert(file.clone(), in_memory_file);
                self.flush_to_wal(&mut manifest, &mut memory_buffer, &mut membuf_size)
                    .await?;
                in_memory_file = manifest.get(file).unwrap().clone();
            }

            self.write_chunk(
                chunk,
                chunk_start,
                tx_id,
                cipher,
                &mut in_memory_file,
                &mut memory_buffer,
                &mut membuf_size,
            )
            .await?;

            chunk_start += chunk.len() as u64;
        }

        self.commit_tx(
            tx_id,
            &mut in_memory_file,
            &mut memory_buffer,
            &mut membuf_size,
        )
        .await;

        manifest.insert(file.clone(), in_memory_file);
        Ok(())
    }

    pub async fn write_chunk(
        &self,
        chunk: &[u8],
        start: u64,
        tx_id: u64,
        cipher: Option<&XChaCha20Poly1305>,
        in_memory_file: &mut InMemoryFile,
        memory_buffer: &mut HashMap<u64, Vec<u8>>,
        membuf_size: &mut usize,
    ) -> Result<(), FsError> {
        let chunk_hashes = self.chunk_hashes.read().await;

        let chunk_hash: [u8; 32] = blake3::hash(chunk).into();
        let chunk_length = chunk.len() as u64;
        let (copy, copy_location) = chunk_hashes
            .get(&chunk_hash)
            .map_or((false, None), |&location| (true, Some(location)));

        let encrypted = match cipher {
            Some(_) => true,
            None => false,
        };

        let proper_position = match copy {
            true => copy_location.unwrap(),
            false => {
                let mem_key = rand::random::<u64>();
                memory_buffer.insert(mem_key, chunk.to_vec());
                *membuf_size += chunk.len();
                ChunkLocation::Memory(mem_key)
            }
        };
        let mem_buffer_meta = Some(MemBufferMeta { copy, tx_id });

        let entry = Chunk {
            start,
            length: chunk_length,
            hash: chunk_hash,
            encrypted,
            location: proper_position,
            mem_buffer_meta,
        };

        // update the in_memory_file directly
        in_memory_file
            .active_txs
            .entry(tx_id)
            .or_default()
            .push(entry);

        Ok(())
    }

    pub async fn read_from_file(
        &self,
        file: &InMemoryFile,
        memory_buffer: &HashMap<u64, Vec<u8>>,
        start: Option<u64>,
        length: Option<u64>,
    ) -> Result<Vec<u8>, FsError> {
        let mut data = Vec::new();
        let mut total_bytes_read = 0;

        // filter chunks based on start and length if they are defined
        let filtered_chunks = if let (Some(start), Some(length)) = (start, length) {
            file.find_chunks_in_range(start, length)
        } else {
            file.chunks.values().cloned().collect()
        };

        for chunk in filtered_chunks {
            let mut read_cache = self.read_cache.write().await;

            let mut chunk_data = if let Some(cached_data) = read_cache.get(&chunk.hash).cloned() {
                cached_data
            } else {
                match chunk.location {
                    ChunkLocation::Memory(memkey) => {
                        let chunk_data = memory_buffer
                            .get(&memkey)
                            .ok_or(FsError::MemoryBufferError {
                                error: format!("No data found for memkey: {}", memkey),
                            })
                            .cloned()?;
                        let _ = read_cache.insert(chunk.hash, chunk_data.clone());
                        chunk_data
                    }
                    ChunkLocation::Wal(offset) => {
                        let mut wal_file = self.wal_file.write().await;
                        let buffer =
                            fetch_from_wal(offset, &mut wal_file, &chunk, &self.cipher).await?;
                        let _ = read_cache.insert(chunk.hash, buffer.clone());
                        buffer
                    }
                    ChunkLocation::ColdStorage(local) => {
                        if local {
                            let path = self.fs_directory_path.join(hex::encode(chunk.hash));
                            let mut buffer =
                                fs::read(path).await.map_err(|e| FsError::IOError {
                                    error: format!("Local Cold read failed: {}", e),
                                })?;
                            if chunk.encrypted {
                                buffer = decrypt(&*self.cipher, &buffer)?;
                            }
                            buffer
                        } else {
                            let file_name = hex::encode(chunk.hash);
                            let (client, bucket) = self.s3_client.as_ref().unwrap();
                            let req = GetObjectRequest {
                                bucket: bucket.clone(),
                                key: file_name.clone(),
                                ..Default::default()
                            };
                            let res = client.get_object(req).await?;
                            let body = res.body.unwrap();
                            let mut stream = body.into_async_read();
                            let mut buffer = Vec::new();
                            stream.read_to_end(&mut buffer).await?;
                            if chunk.encrypted {
                                buffer = decrypt(&*self.cipher, &buffer)?;
                            }
                            let _ = read_cache.insert(chunk.hash, buffer.clone());
                            buffer
                        }
                    }
                }
            };

            // adjust the chunk data based on the start and length
            if let Some(start) = start {
                if start > chunk.start {
                    chunk_data.drain(..(start - chunk.start) as usize);
                }
            }
            if let Some(length) = length {
                let remaining_length = length.saturating_sub(total_bytes_read);
                if remaining_length < chunk_data.len() as u64 {
                    chunk_data.truncate(remaining_length as usize);
                }
                total_bytes_read += chunk_data.len() as u64;
            }

            data.append(&mut chunk_data);
        }

        Ok(data)
    }

    pub async fn read(
        &self,
        file_id: &FileIdentifier,
        start: Option<u64>,
        length: Option<u64>,
    ) -> Result<Vec<u8>, FsError> {
        let file = self.get(file_id).await.ok_or(FsError::NotFound {
            file: file_id.to_uuid().unwrap_or_default(),
        })?;
        let memory_buffer = self.memory_buffer.read().await;
        self.read_from_file(&file, &memory_buffer, start, length)
            .await
    }

    pub async fn write_at(
        &self,
        file_id: &FileIdentifier,
        offset: u64,
        data: &[u8],
    ) -> Result<(), FsError> {
        let mut file = self.get(file_id).await.ok_or(FsError::NotFound {
            file: file_id.to_uuid().unwrap_or_default(),
        })?;
        let mut manifest = self.manifest.write().await;

        let mut memory_buffer = self.memory_buffer.write().await;
        let mut membuf_size = self.membuf_size.write().await;

        let cipher: Option<&XChaCha20Poly1305> = if self.encryption {
            Some(&self.cipher)
        } else {
            None
        };

        let affected_chunks = file.find_chunks_in_range(offset, data.len() as u64);
        let mut data_offset = 0;

        let tx_id = rand::random::<u64>(); // uuid instead?

        let initial_length = file.get_len();
        for chunk in affected_chunks {
            let chunk_data_start = if chunk.start < offset {
                (offset - chunk.start) as usize
            } else {
                0
            };
            let remaining_length = chunk.length as usize - chunk_data_start;
            let remaining_data = data.len() - data_offset;
            let write_length = remaining_length.min(remaining_data);

            // let mut chunk_data = self.read(file_id, Some(start), Some(length)).await?;
            let mut chunk_data = self
                .read_from_file(&file, &memory_buffer, Some(chunk.start), Some(chunk.length))
                .await?;
            chunk_data.resize(
                std::cmp::max(chunk_data_start + write_length, initial_length as usize),
                0,
            ); // extend the chunk data if necessary

            let data_to_write = &data[data_offset..data_offset + write_length];
            chunk_data[chunk_data_start..chunk_data_start + write_length]
                .copy_from_slice(data_to_write);

            if *membuf_size + chunk_data.len() > self.memory_limit {
                manifest.insert(file_id.clone(), file);
                self.flush_to_wal(&mut manifest, &mut memory_buffer, &mut membuf_size)
                    .await?;
                file = manifest.get(file_id).unwrap().clone();
            }

            self.write_chunk(
                &chunk_data,
                chunk.start,
                tx_id,
                cipher,
                &mut file,
                &mut memory_buffer,
                &mut membuf_size,
            )
            .await?;
            data_offset += write_length;
        }

        // if there's still data left to write, create a new chunk
        if data_offset < data.len() {
            let remaining_data = &data[data_offset..];
            let start = file.get_len();

            if *membuf_size + remaining_data.len() > self.memory_limit {
                self.flush_to_wal(&mut manifest, &mut memory_buffer, &mut membuf_size)
                    .await?;
            }

            self.write_chunk(
                remaining_data,
                start,
                tx_id,
                cipher,
                &mut file,
                &mut memory_buffer,
                &mut membuf_size,
            )
            .await?;
        }

        self.commit_tx(tx_id, &mut file, &mut memory_buffer, &mut membuf_size)
            .await;

        manifest.insert(file_id.clone(), file);

        Ok(())
    }

    pub async fn append(&self, file_id: &FileIdentifier, data: &[u8]) -> Result<(), FsError> {
        let file = self.get(file_id).await.ok_or(FsError::NotFound {
            file: file_id.to_uuid().unwrap_or_default(),
        })?;

        let offset = file.get_len();
        self.write_at(file_id, offset, data).await
    }

    //  doublecheck encryption on/off mode with this.
    pub async fn set_length(
        &self,
        file_id: &FileIdentifier,
        new_length: u64,
    ) -> Result<(), FsError> {
        let mut manifest = self.manifest.write().await;
        let mut in_memory_file = manifest
            .get(file_id)
            .ok_or(FsError::NotFound {
                file: file_id.to_uuid().unwrap_or_default(),
            })?
            .clone();

        let mut memory_buffer = self.memory_buffer.write().await;
        let mut membuf_size = self.membuf_size.write().await;

        let cipher: Option<&XChaCha20Poly1305> = if self.encryption {
            Some(&self.cipher)
        } else {
            None
        };

        let file_len = in_memory_file.get_len();

        let tx_id = rand::random::<u64>(); // uuid instead?

        if new_length > file_len {
            // extend with zeroes
            let extension_length = new_length - file_len;
            let extension_data = vec![0; extension_length as usize];

            if *membuf_size + extension_data.len() > self.memory_limit {
                self.flush_to_wal(&mut manifest, &mut memory_buffer, &mut membuf_size)
                    .await?;
            }

            self.write_chunk(
                &extension_data,
                file_len,
                tx_id,
                cipher,
                &mut in_memory_file,
                &mut memory_buffer,
                &mut membuf_size,
            )
            .await?;
        } else if new_length < file_len {
            // truncate
            let affected_chunk = in_memory_file.find_chunks_in_range(new_length, 1);
            if let Some(chunk) = affected_chunk.first() {
                let mut chunk_data = self
                    .read(file_id, Some(chunk.start), Some(chunk.length))
                    .await?;
                chunk_data.truncate((new_length - chunk.start) as usize);

                if *membuf_size + chunk_data.len() > self.memory_limit {
                    self.flush_to_wal(&mut manifest, &mut memory_buffer, &mut membuf_size)
                        .await?;
                }

                self.write_chunk(
                    &chunk_data,
                    chunk.start,
                    tx_id,
                    cipher,
                    &mut in_memory_file,
                    &mut memory_buffer,
                    &mut membuf_size,
                )
                .await?;
            }
            in_memory_file.chunks.retain(|&start, _| start < new_length);
            // doublecheck so that flushing doesn't break:
            let filtered_mem_chunks: Vec<MemChunkIndex> = in_memory_file
                .mem_chunks
                .iter()
                .filter_map(|start| match start {
                    MemChunkIndex::Chunk(start) if start < &new_length => {
                        Some(MemChunkIndex::Chunk(*start))
                    }
                    _ => None,
                })
                .collect();

            in_memory_file.mem_chunks = filtered_mem_chunks;
            in_memory_file
                .wal_chunks
                .retain(|&start| start < new_length);
        }

        self.commit_tx(
            tx_id,
            &mut in_memory_file,
            &mut memory_buffer,
            &mut membuf_size,
        )
        .await;

        manifest.insert(file_id.clone(), in_memory_file);

        Ok(())
    }

    pub async fn flush_to_cold(&self) -> Result<(), FsError> {
        let mut manifest_lock = self.manifest.write().await;
        let mut wal_file = self.wal_file.write().await;
        let mut manifest_file = self.manifest_file.write().await;
        let mut chunk_hashes = self.chunk_hashes.write().await;
        let mut hash_index = self.hash_index.write().await;
        let mut memory_buffer = self.memory_buffer.write().await;
        let mut membuf_size = self.membuf_size.write().await;

        let mut to_flush: Vec<(FileIdentifier, Vec<Chunk>)> = Vec::new();
        for (file_id, in_memory_file) in manifest_lock.iter_mut() {
            let mut chunks_to_flush: Vec<Chunk> = Vec::new();

            for start in &in_memory_file.mem_chunks {
                match start {
                    MemChunkIndex::Chunk(start) => {
                        if let Some(chunk) = in_memory_file.chunks.get(&start) {
                            if matches!(chunk.location, ChunkLocation::Memory(_)) {
                                chunks_to_flush.push(*chunk);
                            }
                        }
                    }
                    MemChunkIndex::CommitTx(_tx_id) => {}
                }
            }

            for &start in &in_memory_file.wal_chunks {
                if let Some(chunk) = in_memory_file.chunks.get(&start) {
                    if matches!(chunk.location, ChunkLocation::Wal(_)) {
                        chunks_to_flush.push(*chunk);
                    }
                }
            }

            // note here, for active_txs continuity. but should our active_txs only be in wal & mem?
            // somewhat of a strong case here for it.
            for txs in &in_memory_file.active_txs {
                for chunk in txs.1 {
                    if matches!(chunk.location, ChunkLocation::Memory(_)) {
                        chunks_to_flush.push(*chunk);
                    }
                }
            }

            if !chunks_to_flush.is_empty() {
                to_flush.push((file_id.clone(), chunks_to_flush));
            }
        }

        for (file_id, chunks) in to_flush.iter() {
            let in_memory_file = manifest_lock.get_mut(file_id).unwrap();
            for chunk in chunks.iter() {
                let total_len = if chunk.encrypted {
                    chunk.length + ENCRYPTION_OVERHEAD as u64
                } else {
                    chunk.length
                };

                let buffer = match &chunk.location {
                    ChunkLocation::Wal(wal_pos) => {
                        // seek to the chunk in the WAL file
                        wal_file.seek(SeekFrom::Start(*wal_pos)).await?;
                        let mut buffer: [u8; 8] = [0; 8];
                        wal_file.read_exact(&mut buffer).await?;
                        let metadata_len = u64::from_le_bytes(buffer) as i64;
                        wal_file
                            .seek(SeekFrom::Current(metadata_len))
                            .await
                            .map_err(|e| FsError::IOError {
                                error: format!("Local WAL seek failed: {}", e),
                            })?;
                        let mut buffer = vec![0u8; total_len as usize];
                        wal_file.read_exact(&mut buffer).await?;

                        buffer
                    }
                    ChunkLocation::Memory(memkey) => {
                        // convert mem_pos and length to usize
                        let mut buffer = memory_buffer
                            .get(&memkey)
                            .ok_or(FsError::MemoryBufferError {
                                error: format!("No data found for memkey: {}", memkey),
                            })?
                            .clone();

                        if chunk.encrypted {
                            buffer = encrypt(&*self.cipher, &buffer)?;
                        }
                        buffer
                    }
                    _ => vec![],
                };

                // write the chunk data to a new file in the filesystem
                if self.cloud_enabled {
                    let file_name = hex::encode(&chunk.hash);
                    let (client, bucket) = self.s3_client.as_ref().unwrap();
                    let req = PutObjectRequest {
                        bucket: bucket.clone(),
                        key: file_name.clone(),
                        body: Some(StreamingBody::from(buffer)),
                        ..Default::default()
                    };
                    client.put_object(req).await?;
                } else {
                    let path = self.fs_directory_path.join(hex::encode(&chunk.hash));
                    fs::write(path, buffer).await?;
                }
                // add a manifest entry with the new hash and removed wal_position
                in_memory_file.chunks.insert(
                    chunk.start,
                    Chunk {
                        start: chunk.start,
                        hash: chunk.hash,
                        length: chunk.length,
                        location: ChunkLocation::ColdStorage(!self.cloud_enabled),
                        encrypted: chunk.encrypted,
                        mem_buffer_meta: None,
                    },
                );
                chunk_hashes.insert(chunk.hash, ChunkLocation::ColdStorage(!self.cloud_enabled));
            }
            in_memory_file.mem_chunks.clear();
            in_memory_file.wal_chunks.clear();

            // chunks have been flushed, let's add a manifest entry.
            let entry = ManifestRecord::Backup(BackupEntry {
                file: file_id.clone(),
                chunks: in_memory_file
                    .chunks
                    .iter()
                    .map(|(&start, chunk)| {
                        let local = match chunk.location {
                            ChunkLocation::ColdStorage(local) => local,
                            _ => true, // WAL is always local
                        };
                        (chunk.hash, start, chunk.length, chunk.encrypted, local)
                    })
                    .collect::<Vec<_>>(),
            });

            let serialized_entry = rmp_serde::encode::to_vec(&entry).unwrap();
            let entry_length = serialized_entry.len() as u64;

            let mut buffer = Vec::new();
            buffer.extend_from_slice(&entry_length.to_le_bytes());
            buffer.extend_from_slice(&serialized_entry);

            manifest_file.write_all(&buffer).await?;
            if self.cloud_enabled {
                let (client, bucket) = self.s3_client.as_ref().unwrap();
                let mut buffer = Vec::new();
                manifest_file.seek(SeekFrom::Start(0)).await?;
                manifest_file.read_to_end(&mut buffer).await?;

                let req = PutObjectRequest {
                    bucket: bucket.clone(),
                    key: "manifest.bin".to_string(),
                    body: Some(StreamingBody::from(buffer)),
                    ..Default::default()
                };
                client.put_object(req).await?;
            }
            hash_index.insert(in_memory_file.hash(), file_id.clone());
        }
        // clear the WAL file and memory buffer
        manifest_file.sync_all().await?;
        wal_file.set_len(0).await?;
        memory_buffer.clear();
        *membuf_size = 0;
        Ok(())
    }

    pub async fn delete(&self, file_id: &FileIdentifier) -> Result<(), FsError> {
        // add a delete entry to the manifest
        let entry = ManifestRecord::Delete(file_id.clone());
        let serialized_entry = rmp_serde::encode::to_vec(&entry).unwrap();
        let entry_length = serialized_entry.len() as u64;
        let mut manifest_file = self.manifest_file.write().await;

        manifest_file.write_all(&entry_length.to_le_bytes()).await?;
        manifest_file.write_all(&serialized_entry).await?;
        // manifest_file.sync_all().await?;

        // remove the file from the manifest
        let mut manifest = self.manifest.write().await;
        manifest.remove(file_id);

        Ok(())
    }

    pub async fn _cleanup(&self) -> Result<(), FsError> {
        let in_use_hashes = self._get_chunk_hashes().await;

        // loop through all chunks on disk
        let mut entries = fs::read_dir(&self.fs_directory_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let file_name = path.file_name().and_then(|os_str| os_str.to_str());

            if let Some(file_name) = file_name {
                if file_name == "manifest.bin" || file_name == "wal.bin" {
                    continue;
                }

                if let Ok(vec) = hex::decode(file_name) {
                    let hash: [u8; 32] = match vec[..].try_into() {
                        Ok(array) => array,
                        Err(_) => continue, // jf the conversion fails, skip
                    };

                    // if the chunk is used, delete it
                    if !in_use_hashes.contains(&hash) {
                        let _ = fs::remove_file(path).await;
                    }
                }
            }
        }

        Ok(())
    }
}

impl Clone for Manifest {
    fn clone(&self) -> Self {
        Self {
            manifest: Arc::clone(&self.manifest),
            chunk_hashes: Arc::clone(&self.chunk_hashes),
            hash_index: Arc::clone(&self.hash_index),
            manifest_file: Arc::clone(&self.manifest_file),
            wal_file: Arc::clone(&self.wal_file),
            fs_directory_path: self.fs_directory_path.clone(),
            flush_cold_freq: self.flush_cold_freq,
            memory_buffer: Arc::clone(&self.memory_buffer),
            membuf_size: Arc::clone(&self.membuf_size),
            read_cache: Arc::clone(&self.read_cache),
            memory_limit: self.memory_limit,
            chunk_size: self.chunk_size,
            cipher: Arc::clone(&self.cipher),
            encryption: self.encryption,
            cloud_enabled: self.cloud_enabled,
            s3_client: self.s3_client.clone(),
        }
    }
}

async fn load_manifest(
    manifest_file: &mut fs::File,
    manifest: &mut HashMap<FileIdentifier, InMemoryFile>,
) -> Result<(), io::Error> {
    let mut current_position = 0;

    loop {
        // Seek to the current position
        manifest_file
            .seek(SeekFrom::Start(current_position))
            .await?;

        // Read length of the serialized metadata
        let mut length_buffer = [0u8; 8];
        let read_size: usize = manifest_file.read(&mut length_buffer).await?;

        if read_size < 8 {
            // Not enough data left to read metadata length, break out of the loop
            break;
        }
        let metadata_length = u64::from_le_bytes(length_buffer) as usize;

        // Read serialized metadata
        let mut metadata_buffer = vec![0u8; metadata_length];
        manifest_file.read_exact(&mut metadata_buffer).await?;
        let record_metadata: Result<ManifestRecord, _> =
            rmp_serde::decode::from_slice(&metadata_buffer);

        match record_metadata {
            Ok(ManifestRecord::Backup(entry)) => {
                manifest.insert(
                    entry.file,
                    InMemoryFile {
                        chunks: entry
                            .chunks
                            .iter()
                            .map(|(hash, start, length, encrypted, local)| {
                                (
                                    *start,
                                    Chunk {
                                        start: *start,
                                        hash: *hash,
                                        length: *length,
                                        location: ChunkLocation::ColdStorage(*local),
                                        encrypted: *encrypted,
                                        mem_buffer_meta: None,
                                    },
                                )
                            })
                            .collect(),
                        active_txs: HashMap::new(),
                        mem_chunks: Vec::new(),
                        wal_chunks: Vec::new(),
                    },
                );
                // move to the next position after the metadata,
                current_position += 8 + metadata_length as u64;
            }
            Ok(ManifestRecord::Delete(delete)) => {
                manifest.remove(&delete);
                current_position += 8 + metadata_length as u64;
            }

            Err(_) => {
                // faulty entry, remove.
                break;
            }
        }
    }
    // truncate the manifest file to the current position
    manifest_file.set_len(current_position).await?;
    Ok(())
}

async fn _compact_manifest(
    manifest: &mut HashMap<FileIdentifier, InMemoryFile>,
) -> Result<(), io::Error> {
    let mut temp_manifest = fs::File::create("manifest.temp").await?;

    let mut buffer = Vec::new();
    for (file_id, in_memory_file) in manifest.iter() {
        let entry = ManifestRecord::Backup(BackupEntry {
            file: file_id.clone(),
            chunks: in_memory_file
                .chunks
                .iter()
                .map(|(&start, chunk)| {
                    let local = match chunk.location {
                        ChunkLocation::ColdStorage(local) => local,
                        _ => true, // WAL is always local
                    };
                    (chunk.hash, start, chunk.length, chunk.encrypted, local)
                })
                .collect::<Vec<_>>(),
        });

        let serialized_entry = rmp_serde::encode::to_vec(&entry).unwrap();
        let entry_length = serialized_entry.len() as u64;

        buffer.extend_from_slice(&entry_length.to_le_bytes());
        buffer.extend_from_slice(&serialized_entry);
    }
    temp_manifest.write_all(&buffer).await?;
    temp_manifest.sync_all().await?;
    fs::rename("manifest.temp", "manifest.bin").await?;

    Ok(())
}

async fn load_wal(
    wal_file: &mut fs::File,
    manifest: &mut HashMap<FileIdentifier, InMemoryFile>,
) -> Result<(), io::Error> {
    let mut current_position = 0;

    let mut tx_chunks: HashMap<
        u64,
        (
            FileIdentifier,
            Vec<(u64, [u8; 32], u64, ChunkLocation, bool)>,
        ),
    > = HashMap::new();

    loop {
        // seek to the current position
        wal_file.seek(SeekFrom::Start(current_position)).await?;

        // read length of the serialized metadata
        let mut length_buffer = [0u8; 8];
        let read_size: usize = wal_file.read(&mut length_buffer).await?;

        if read_size < 8 {
            // not enough data left to read metadata length, break out of the loop
            break;
        }
        let record_length = u64::from_le_bytes(length_buffer) as usize;
        //  println!("Record length: {}", record_length);

        // read serialized metadata
        let mut record_buffer = vec![0u8; record_length];
        match wal_file.read_exact(&mut record_buffer).await {
            Ok(_) => {
                let record: Result<WALRecord, _> = rmp_serde::decode::from_slice(&record_buffer);
                match record {
                    Ok(WALRecord::CommitTx(tx_id)) => {
                        if let Some((file_id, chunks)) = tx_chunks.remove(&tx_id) {
                            let in_memory_file = manifest.entry(file_id).or_default();
                            for (start, hash, length, location, encrypted) in chunks {
                                in_memory_file.chunks.insert(
                                    start,
                                    Chunk {
                                        start,
                                        hash,
                                        length,
                                        location,
                                        encrypted,
                                        mem_buffer_meta: None,
                                    },
                                );
                                if let ChunkLocation::Wal(_) = location {
                                    in_memory_file.wal_chunks.push(start);
                                }
                            }
                        }
                        current_position += 8 + record_length as u64;
                    }
                    Ok(WALRecord::Chunk(entry)) => {
                        //  let data_position: u64 = current_position + 8 + record_length as u64;
                        let data_length = entry.length;

                        let location = match entry.copy_location {
                            Some(location) => location,
                            None => ChunkLocation::Wal(current_position),
                        };

                        let chunks = tx_chunks
                            .entry(entry.tx_id)
                            .or_insert((entry.file, Vec::new()));
                        chunks.1.push((
                            entry.start,
                            entry.chunk_hash,
                            entry.length,
                            location,
                            entry.encrypted,
                        ));

                        // if it's a copy, we don't have to skip + data_length to get to the next position
                        // if encrypted data, add encryption overhead (nonce 24 + tag 16)
                        current_position += 8 + record_length as u64;
                        if entry.copy_location.is_none() {
                            current_position += data_length;
                            if entry.encrypted {
                                current_position += ENCRYPTION_OVERHEAD as u64;
                            }
                        }
                    }
                    Err(_) => {
                        //  println!("failed to deserialize WALRecord.");
                        break;
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                println!("Encountered an incomplete record. Truncating the file.");
                break;
            }
            Err(e) => return Err(e),
        }
    }

    // truncate the WAL file to the current position
    wal_file.set_len(current_position).await?;
    Ok(())
}

async fn verify_manifest(
    manifest: &mut HashMap<FileIdentifier, InMemoryFile>,
    chunk_hashes: &mut HashMap<[u8; 32], ChunkLocation>,
    hash_index: &mut HashMap<[u8; 32], FileIdentifier>,
) -> tokio::io::Result<()> {
    for (file, in_memory_file) in manifest.iter_mut() {
        let file_hash = in_memory_file.hash();

        for chunk in in_memory_file.chunks.values() {
            chunk_hashes.insert(chunk.hash, chunk.location);
        }
        hash_index.insert(file_hash, file.clone());
    }
    Ok(())
}

/// HELPERS

async fn fetch_from_wal(
    offset: u64,
    wal_file: &mut fs::File,
    chunk: &Chunk,
    cipher: &XChaCha20Poly1305,
) -> Result<Vec<u8>, FsError> {
    wal_file
        .seek(SeekFrom::Start(offset))
        .await
        .map_err(|e| FsError::IOError {
            error: format!("Local WAL seek failed: {}", e),
        })?;
    let len = if chunk.encrypted {
        chunk.length + ENCRYPTION_OVERHEAD as u64
    } else {
        chunk.length
    };

    let mut buffer: [u8; 8] = [0; 8];
    wal_file
        .read_exact(&mut buffer)
        .await
        .map_err(|e| FsError::IOError {
            error: format!("Local WAL read failed: {}", e),
        })?;
    let metadata_len = u64::from_le_bytes(buffer) as i64;
    wal_file
        .seek(SeekFrom::Current(metadata_len))
        .await
        .map_err(|e| FsError::IOError {
            error: format!("Local WAL seek failed: {}", e),
        })?;
    let mut buffer = vec![0u8; len as usize];
    wal_file
        .read_exact(&mut buffer)
        .await
        .map_err(|e| FsError::IOError {
            error: format!("Local WAL read failed: {}", e),
        })?;
    if chunk.encrypted {
        buffer = decrypt(cipher, &buffer)?;
    }
    Ok(buffer)
}

/// serialize helpers

async fn serialize_chunk(
    file: &FileIdentifier,
    chunk: &[u8],
    start: u64,
    tx_id: u64,
    encrypted: bool,
    copy_location: Option<ChunkLocation>,
    cipher: &XChaCha20Poly1305,
) -> Result<Vec<u8>, FsError> {
    let chunk_hash: [u8; 32] = blake3::hash(chunk).into();
    let chunk_length = chunk.len() as u64;

    let entry = ChunkEntry {
        file: file.clone(),
        tx_id,
        start,
        length: chunk_length,
        chunk_hash,
        encrypted,
        copy_location,
    };

    // serialize the metadata
    let serialized_metadata = rmp_serde::encode::to_vec(&WALRecord::Chunk(entry)).unwrap();
    let metadata_length = serialized_metadata.len() as u64;

    let mut buffer = Vec::new();
    buffer.extend_from_slice(&metadata_length.to_le_bytes());
    buffer.extend_from_slice(&serialized_metadata);

    if encrypted {
        let ciphertext = encrypt(cipher, chunk)?;
        buffer.extend_from_slice(&ciphertext);
    } else {
        buffer.extend_from_slice(chunk);
    }

    Ok(buffer)
}

async fn serialize_commit(tx_id: u64) -> Result<Vec<u8>, FsError> {
    let mut buffer = Vec::new();
    let commit_tx_record = WALRecord::CommitTx(tx_id);
    let serialized_commit_tx = rmp_serde::encode::to_vec(&commit_tx_record).unwrap();
    let commit_tx_length = serialized_commit_tx.len() as u64;

    buffer.extend_from_slice(&commit_tx_length.to_le_bytes());
    buffer.extend_from_slice(&serialized_commit_tx);

    Ok(buffer)
}

fn generate_nonce() -> [u8; 24] {
    let mut nonce = [0u8; 24];
    //  todo change to OsRng
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

fn encrypt(cipher: &XChaCha20Poly1305, bytes: &[u8]) -> Result<Vec<u8>, FsError> {
    let nonce = generate_nonce();
    let ciphertext = cipher.encrypt(XNonce::from_slice(&nonce), bytes)?;
    Ok([nonce.to_vec(), ciphertext].concat())
}

fn decrypt(cipher: &XChaCha20Poly1305, bytes: &[u8]) -> Result<Vec<u8>, FsError> {
    let nonce = XNonce::from_slice(&bytes[..24]);
    let plaintext = cipher.decrypt(nonce, &bytes[24..])?;
    Ok(plaintext)
}

fn parse_s3_config(config: S3Config) -> Result<(S3Client, String), FsError> {
    let region = Region::Custom {
        name: config.region.clone(),
        endpoint: config.endpoint.clone(),
    };

    let client = S3Client::new_with(
        rusoto_core::HttpClient::new().expect("failed to create request dispatcher"),
        rusoto_credential::StaticProvider::new_minimal(config.access_key, config.secret_key),
        region,
    );
    Ok((client, config.bucket))
}

impl From<std::io::Error> for FsError {
    fn from(error: std::io::Error) -> Self {
        FsError::IOError {
            error: error.to_string(),
        }
    }
}

impl From<aes_gcm::aead::Error> for FsError {
    fn from(error: aes_gcm::aead::Error) -> Self {
        FsError::EncryptionError {
            error: error.to_string(),
        }
    }
}

impl From<RusotoError<PutObjectError>> for FsError {
    fn from(error: RusotoError<PutObjectError>) -> Self {
        FsError::S3Error {
            error: format!("PUT error: {}", error),
        }
    }
}

impl From<RusotoError<GetObjectError>> for FsError {
    fn from(error: RusotoError<GetObjectError>) -> Self {
        FsError::S3Error {
            error: format!("GET error: {}", error),
        }
    }
}

impl From<RusotoError<DeleteObjectError>> for FsError {
    fn from(error: RusotoError<DeleteObjectError>) -> Self {
        FsError::S3Error {
            error: format!("DELETE error: {}", error),
        }
    }
}

impl From<RusotoError<ListObjectsV2Error>> for FsError {
    fn from(error: RusotoError<ListObjectsV2Error>) -> Self {
        FsError::S3Error {
            error: format!("LIST error: {}", error),
        }
    }
}
