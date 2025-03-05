use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use anyhow::Result;
use memmap2::{Mmap, MmapOptions};
use prost::Message;

/// A replica log implementation using memory-mapped files for readers
/// and standard file I/O for the writer.
/// Uses Protocol Buffers for serialization.
/// 
/// This implementation is thread-safe and allows for concurrent
/// reading but only one writer thread.
/// 
/// Reads won't block the writer and the writer won't block readers.
#[derive(Clone)]
pub struct ReplicaLogAppender<T> {
    // RwLock ensures single writer, multiple readers
    inner: Arc<RwLock<ReplicaLogInner<T>>>,
}

struct ReplicaLogInner<T> {
    file: File,
    file_size: u64,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> ReplicaLogAppender<T>
where
    T: Message + Default + Send + Sync + 'static,
{
    /// Opens a replica log at the specified path
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        
        // Open file with append mode to ensure we always write to the end
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;
            
        // Get the initial file size
        let file_size = file.metadata()?.len();
            
        let inner = ReplicaLogInner {
            file,
            file_size,
            _phantom: std::marker::PhantomData,
        };
        
        Ok(Self {
            inner: Arc::new(RwLock::new(inner)),
        })
    }
    
    /// Writes an entry to the log
    /// 
    /// This operation appends the entry to the end of the log.
    /// Only one writer can write at a time, but readers won't block the writer.
    pub fn append(&self, entry: &T) -> Result<()> {
        // Serialize the entry with Protocol Buffers
        let mut serialized = Vec::with_capacity(entry.encoded_len());
        entry.encode(&mut serialized).map_err(|e| anyhow::anyhow!("Failed to encode message: {}", e))?;
            
        // Write with length prefix
        let len = serialized.len() as u32;
        let len_bytes = len.to_le_bytes();
        
        // Get write lock for the file
        let mut inner = self.inner.write().unwrap();
        
        // Write length followed by serialized data
        inner.file.write_all(&len_bytes)?;
        inner.file.write_all(&serialized)?;
        inner.file.sync_data()?;
        
        // Update file size
        inner.file_size += (len_bytes.len() + serialized.len()) as u64;
        
        Ok(())
    }
}

/// Returns an iterator over all entries in the log
/// 
/// This uses memory mapping for efficient reading, and multiple readers
/// can call this method concurrently without blocking a writer.
pub fn get_replica_log_iterator<T>(file_path:PathBuf) -> Result<MmapLogIterator<T>> {
    // Get read lock to safely access the path and current file size
    let file_size = File::open(&file_path)?.metadata()?.len();
    
    // Create a new file handle specifically for this reader
    let file = OpenOptions::new()
        .read(true)
        .open(&file_path)?;
        
    // Create memory map for the file or re-use existing one
    let mmap = unsafe {
        MmapOptions::new()
            .map(&file)?
    };
    
    Ok(MmapLogIterator {
        mmap,
        position: 0,
        file_size,
        path: file_path.clone(),
        _phantom: std::marker::PhantomData,
    })
}
/// An iterator over the entries in a log using memory mapping
pub struct MmapLogIterator<T> {
    mmap: Mmap,
    position: usize,
    file_size: u64,
    path: PathBuf,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> Iterator for MmapLogIterator<T>
where
    T: Message + Default,
{
    type Item = Result<T>;
    
    fn next(&mut self) -> Option<Self::Item> {
        // Check if we've reached the end of the file
        if self.position >= self.mmap.len() {
            return None;
        }
        
        // Ensure we have enough bytes to read the length prefix
        if self.position + 4 > self.mmap.len() {
            return Some(Err(anyhow::anyhow!("not enough bytes for length prefix")));
        }
        
        // Read length prefix from memory map
        let len_bytes = [
            self.mmap[self.position],
            self.mmap[self.position + 1],
            self.mmap[self.position + 2],
            self.mmap[self.position + 3],
        ];
        self.position += 4;
        
        let len = u32::from_le_bytes(len_bytes) as usize;
        
        // Ensure we have enough bytes for the entry
        if self.position + len > self.mmap.len() {
            // Rewind position to where we started
            self.position -= 4;
            return Some(Err(anyhow::anyhow!("not enough bytes for entry")));
        }
        
        // Extract entry data from memory map
        let entry_data = &self.mmap[self.position..self.position + len];
        self.position += len;
        
        // Deserialize the entry using Protocol Buffers
        let mut entry = T::default();
        match entry.merge(entry_data) {
            Ok(_) => Some(Ok(entry)),
            Err(e) => Some(Err(anyhow::anyhow!("Failed to decode message: {}", e))),
        }
    }
}

/// Get a refreshed view of the log when the file has grown
impl<T> MmapLogIterator<T> {
    /// Refreshes the memory map to see newer entries
    /// 
    /// This is useful when the log file has grown since this iterator was created.
    pub fn refresh(&mut self) -> Result<()> {
        // Save our position
        let position = self.position;
        
        // Re-open the file using the stored path
        let file = File::open(&self.path)?;
        
        // Get new file size
        let file_size = file.metadata()?.len();
        
        // Only refresh if the file has grown
        if file_size > self.file_size {
            // Create a new memory map
            let mmap = unsafe {
                MmapOptions::new()
                    .map(&file)?
            };
            
            // Replace our mmap with the new one
            self.mmap = mmap;
            self.file_size = file_size;
        }
        
        // Restore position if it's still valid
        self.position = position.min(self.mmap.len());
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;
    use std::sync::Arc;
    use std::thread;
    use tempfile::tempdir;
    
    #[derive(Clone, PartialEq, Message)]
    struct TestEntry {
        #[prost(uint64, tag = "1")]
        id: u64,
        #[prost(string, tag = "2")]
        data: String,
    }
    
    #[test]
    fn test_basic_write_read() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        
        let log = ReplicaLogAppender::<TestEntry>::open(&log_path).unwrap();
        
        // Write some entries
        log.append(&TestEntry { id: 1, data: "first".to_string() }).unwrap();
        log.append(&TestEntry { id: 2, data: "second".to_string() }).unwrap();
        log.append(&TestEntry { id: 3, data: "third".to_string() }).unwrap();
        
        // Read them back
        let entries: Vec<TestEntry> = get_replica_log_iterator(log_path)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
            
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].id, 1);
        assert_eq!(entries[1].id, 2);
        assert_eq!(entries[2].id, 3);
    }
    
    #[test]
    fn test_concurrent_reads() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("concurrent_reads.log");
        
        let log = Arc::new(ReplicaLogAppender::<TestEntry>::open(&log_path).unwrap());
        
        // Pre-populate with some entries
        for i in 0..100 {
            log.append(&TestEntry { 
                id: i, 
                data: format!("entry-{}", i),
            }).unwrap();
        }
        
        // Create a bunch of reader threads
        let mut handles = vec![];
        let num_threads = 10;
        
        for t in 0..num_threads {
            let log_path = log_path.clone();
            let handle = thread::spawn(move || {
                // Each thread reads all entries
                let entries: Vec<TestEntry> = get_replica_log_iterator::<TestEntry>(log_path)
                    .unwrap()
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                    
                assert_eq!(entries.len(), 100);
                
                // Verify some entries
                assert_eq!(entries[0].id, 0);
                assert_eq!(entries[50].id, 50);
                assert_eq!(entries[99].id, 99);
                
                // Thread ID for debugging
                t as usize
            });
            handles.push(handle);
        }
        
        // Wait for all readers to finish and collect results
        let results: Vec<usize> = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();
            
        // Should have results from all threads
        assert_eq!(results.len(), num_threads);
    }
    
    #[test]
    fn test_refresh_iterator() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("refresh.log");
        
        let log = ReplicaLogAppender::<TestEntry>::open(&log_path).unwrap();
        
        // Write initial entries
        for i in 0..10 {
            log.append(&TestEntry { 
                id: i, 
                data: format!("initial-{}", i),
            }).unwrap();
        }
        
        // Create an iterator
        let mut iter = get_replica_log_iterator::<TestEntry>(log_path.clone()).unwrap();
        
        // Read first 5 entries
        let initial_entries: Vec<TestEntry> = iter
            .by_ref()
            .take(5)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
            
        assert_eq!(initial_entries.len(), 5);
        
        // Write more entries
        for i in 10..20 {
            log.append(&TestEntry { 
                id: i, 
                data: format!("additional-{}", i),
            }).unwrap();
        }
        
        // Iterator should still be at entry 6 without refresh
        let more_entries: Vec<TestEntry> = iter
            .by_ref()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
            
        assert_eq!(more_entries.len(), 5); // Only the remaining initial entries
        
        // Refresh the iterator
        iter.refresh().unwrap();
        
        // Read the new entries
        let new_entries: Vec<TestEntry> = iter
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
            
        assert_eq!(new_entries.len(), 10); // The additional entries
        assert_eq!(new_entries[0].id, 10);
        assert_eq!(new_entries[9].id, 19);
    }
    
    #[test]
    fn test_reads_dont_block_writes() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("read_write.log");
        
        let log = Arc::new(ReplicaLogAppender::<TestEntry>::open(&log_path).unwrap());
        
        // Pre-populate with some entries
        for i in 0..10 {
            log.append(&TestEntry { 
                id: i, 
                data: format!("initial-{}", i),
            }).unwrap();
        }
        
        // Start multiple reader threads that read continuously
        let mut reader_handles = vec![];
        let reader_count = 5;
        
        for _ in 0..reader_count {
            let log_path = log_path.clone();
            let handle = thread::spawn(move || {
                // Read entries in a loop
                for _ in 0..10 {
                    let _entries: Vec<TestEntry> = get_replica_log_iterator::<TestEntry>(log_path.clone())
                        .unwrap()
                        .collect::<Result<Vec<_>, _>>()
                        .unwrap();
                    
                    // Small sleep to simulate processing
                    thread::sleep(std::time::Duration::from_millis(5));
                }
            });
            reader_handles.push(handle);
        }
        
        // Writer thread keeps writing while readers are active
        let writer_log = Arc::clone(&log);
        let writer_handle = thread::spawn(move || {
            for i in 10..60 {
                let entry = TestEntry {
                    id: i,
                    data: format!("concurrent-{}", i),
                };
                writer_log.append(&entry).unwrap();
                
                // Small sleep to pace the writes
                thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        
        // Wait for writer to finish
        writer_handle.join().unwrap();
        
        // Wait for all readers to finish
        for handle in reader_handles {
            handle.join().unwrap();
        }
        
        // Verify all entries were written
        let final_entries: Vec<TestEntry> = get_replica_log_iterator::<TestEntry>(log_path)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
            
        // Should have all 60 entries
        assert_eq!(final_entries.len(), 60);
        
        // Verify first and last entries
        assert_eq!(final_entries[0].id, 0);
        assert_eq!(final_entries[59].id, 59);
    }
}