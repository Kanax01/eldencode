use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::FileExt;
use std::path::PathBuf;

/// Errors a memory access can produce. `Permission` is pulled out because on
/// Linux it almost always means the kernel refuses cross-process access
/// (yama ptrace_scope), not that the pointer was bad.
#[derive(Debug)]
pub enum MemError {
    Permission,
    Unmapped,
    Io(io::Error),
}

impl std::fmt::Display for MemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemError::Permission => write!(f, "permission denied by the kernel"),
            MemError::Unmapped => write!(f, "address is not mapped"),
            MemError::Io(err) => write!(f, "{err}"),
        }
    }
}

/// Read/write access to another process. Elden Ring under Proton is an ordinary
/// Linux process (a Wine-hosted PE image), so `/proc/<pid>/mem` plus
/// process_vm_readv/writev is enough — no driver and no Windows API involved.
pub struct Memory {
    pid: i32,
    file: File,
}

impl Memory {
    pub fn open(pid: i32) -> Result<Memory, MemError> {
        let path = PathBuf::from(format!("/proc/{pid}/mem"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|err| match err.kind() {
                io::ErrorKind::PermissionDenied => MemError::Permission,
                _ => MemError::Io(err),
            })?;
        Ok(Memory { pid, file })
    }

    pub fn read(&self, addr: u64, buf: &mut [u8]) -> Result<(), MemError> {
        if buf.is_empty() {
            return Ok(());
        }
        let local = libc::iovec {
            iov_base: buf.as_mut_ptr() as *mut libc::c_void,
            iov_len: buf.len(),
        };
        let remote = libc::iovec {
            iov_base: addr as *mut libc::c_void,
            iov_len: buf.len(),
        };
        let got = unsafe { libc::process_vm_readv(self.pid, &local, 1, &remote, 1, 0) };
        if got == buf.len() as isize {
            return Ok(());
        }
        if got < 0 {
            let errno = io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EIO);
            if matches!(errno, libc::EPERM | libc::EACCES) {
                return Err(MemError::Permission);
            }
        }
        // Partial reads happen at region edges; /proc/pid/mem handles those.
        self.file
            .read_exact_at(buf, addr)
            .map_err(|err| match err.kind() {
                io::ErrorKind::PermissionDenied => MemError::Permission,
                io::ErrorKind::UnexpectedEof => MemError::Unmapped,
                _ => MemError::Io(err),
            })
    }

    pub fn write(&self, addr: u64, buf: &[u8]) -> Result<(), MemError> {
        if buf.is_empty() {
            return Ok(());
        }
        let local = libc::iovec {
            iov_base: buf.as_ptr() as *mut libc::c_void,
            iov_len: buf.len(),
        };
        let remote = libc::iovec {
            iov_base: addr as *mut libc::c_void,
            iov_len: buf.len(),
        };
        let put = unsafe { libc::process_vm_writev(self.pid, &local, 1, &remote, 1, 0) };
        if put == buf.len() as isize {
            return Ok(());
        }
        if put < 0 {
            let errno = io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EIO);
            if matches!(errno, libc::EPERM | libc::EACCES) {
                return Err(MemError::Permission);
            }
        }
        self.file
            .write_all_at(buf, addr)
            .map_err(|err| match err.kind() {
                io::ErrorKind::PermissionDenied => MemError::Permission,
                _ => MemError::Io(err),
            })
    }

    pub fn read_bytes<const N: usize>(&self, addr: u64) -> Result<[u8; N], MemError> {
        let mut buf = [0u8; N];
        self.read(addr, &mut buf)?;
        Ok(buf)
    }

    pub fn read_u8(&self, addr: u64) -> Result<u8, MemError> {
        Ok(self.read_bytes::<1>(addr)?[0])
    }

    pub fn read_u32(&self, addr: u64) -> Result<u32, MemError> {
        Ok(u32::from_le_bytes(self.read_bytes::<4>(addr)?))
    }

    pub fn read_i32(&self, addr: u64) -> Result<i32, MemError> {
        Ok(i32::from_le_bytes(self.read_bytes::<4>(addr)?))
    }

    pub fn read_u64(&self, addr: u64) -> Result<u64, MemError> {
        Ok(u64::from_le_bytes(self.read_bytes::<8>(addr)?))
    }

    pub fn read_f32(&self, addr: u64) -> Result<f32, MemError> {
        Ok(f32::from_le_bytes(self.read_bytes::<4>(addr)?))
    }

    pub fn write_u8(&self, addr: u64, value: u8) -> Result<(), MemError> {
        self.write(addr, &[value])
    }

    pub fn write_u32(&self, addr: u64, value: u32) -> Result<(), MemError> {
        self.write(addr, &value.to_le_bytes())
    }

    pub fn write_f32(&self, addr: u64, value: f32) -> Result<(), MemError> {
        self.write(addr, &value.to_le_bytes())
    }
}
