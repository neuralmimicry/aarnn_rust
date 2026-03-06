//! # Shared Memory (SHM) Communication
//!
//! This module provides a high-performance, zero-copy inter-process communication (IPC)
//! mechanism using memory-mapped files and a ring-buffer architecture.
//!
//! It is designed to facilitate high-bandwidth data exchange between the neuromorphic
//! engine and external simulators or robot controllers running on the same machine.
//!
//! ## Implementation Status: **Experimental**
//! Current version provides the skeleton for memory mapping and header management.
//! Multi-process synchronization primitives (atomics) are planned for future phases.

#![cfg(feature = "shmem")]

use std::fs::File;
use std::io;
use std::path::Path;
// use std::sync::atomic::{AtomicU64, Ordering}; (removed)

use memmap2::{MmapMut, MmapOptions};

/// Header placed at the start of a shared memory region.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RingBufferHeader {
    /// Capacity of the ring in bytes (payload area).
    pub capacity: u64,
    /// Producer and consumer positions in bytes since start (monotonic).
    pub producer_position: u64,
    pub consumer_position: u64,
}

impl Default for RingBufferHeader {
    fn default() -> Self { Self { capacity: 0, producer_position: 0, consumer_position: 0 } }
}

/// Shared-memory ring buffer mapping.
pub struct ShmRingBuffer {
    _file: File,
    _mmap: MmapMut,
    /// Cached header view (updates mirrored to mmap on flush/write).
    header: RingBufferHeader,
    /// Offsets within the mapping.
    _header_offset: usize,
    _data_offset: usize,
}

impl ShmRingBuffer {
    /// Create and initialize a new shared-memory ring buffer backed by a file.
    ///
    /// For initial version we use a regular file path to simplify demoing.
    /// Future iteration can add memfd/shm_open variants.
    pub fn create<P: AsRef<Path>>(path: P, capacity: usize) -> io::Result<Self> {
        use std::fs::OpenOptions;
        let file = OpenOptions::new().create(true).read(true).write(true).open(path)?;
        let total = std::mem::size_of::<RingBufferHeader>() + capacity;
        file.set_len(total as u64)?;
        let mut mmap = unsafe { MmapOptions::new().len(total).map_mut(&file)? };
        let hdr = RingBufferHeader { capacity: capacity as u64, ..Default::default() };
        // Write header
        let hdr_bytes = unsafe {
            std::slice::from_raw_parts((&hdr as *const RingBufferHeader) as *const u8, std::mem::size_of::<RingBufferHeader>())
        };
        mmap[..hdr_bytes.len()].copy_from_slice(hdr_bytes);
        Ok(Self { _file: file, _mmap: mmap, header: hdr, _header_offset: 0, _data_offset: hdr_bytes.len() })
    }

    /// Open an existing ring buffer file and map it.
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        use std::fs::OpenOptions;
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let len = file.metadata()?.len() as usize;
        if len < std::mem::size_of::<RingBufferHeader>() { return Err(io::Error::new(io::ErrorKind::InvalidData, "shmem too small")); }
        let mmap = unsafe { MmapOptions::new().len(len).map_mut(&file)? };
        // Read header
        let mut hdr = RingBufferHeader::default();
        let dst = unsafe { std::slice::from_raw_parts_mut((&mut hdr as *mut RingBufferHeader) as *mut u8, std::mem::size_of::<RingBufferHeader>()) };
        dst.copy_from_slice(&mmap[..dst.len()]);
        Ok(Self { _file: file, _mmap: mmap, header: hdr, _header_offset: 0, _data_offset: std::mem::size_of::<RingBufferHeader>() })
    }

    /// Returns the payload capacity in bytes.
    pub fn capacity(&self) -> usize { self.header.capacity as usize }

    /// Try to write a single frame (bytes) into the ring.
    /// Returns number of bytes written or an error if insufficient space.
    pub fn write_frame(&mut self, data: &[u8]) -> io::Result<usize> {
        // Placeholder: future implementation will implement wrap-around copy and
        // atomics to coordinate multiple processes.
        let _ = data;
        Err(io::Error::new(io::ErrorKind::WouldBlock, "not implemented: write_frame"))
    }

    /// Try to read a single frame into out. Returns bytes read or WouldBlock if none.
    pub fn read_frame(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let _ = out;
        Err(io::Error::new(io::ErrorKind::WouldBlock, "not implemented: read_frame"))
    }
}
