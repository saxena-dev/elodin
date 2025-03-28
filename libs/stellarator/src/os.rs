use std::io;
#[cfg(not(target_os = "windows"))]
use std::os::fd::{AsFd, AsRawFd, FromRawFd, RawFd};

#[cfg(target_os = "windows")]
use std::os::windows::io::{
    AsHandle, AsRawHandle, AsRawSocket, AsSocket, FromRawHandle, RawHandle,
};

use maitake::time::Clock;

use socket2::Socket;

impl OwnedHandle {
    #[cfg(not(target_os = "windows"))]
    /// Creates a `OwnedHandle` from a RawFd
    ///
    /// # Safety
    /// The user must ensure that no one else holds `RawFd`,
    /// because `OwnedHandle` will close the file-descriptor on drop
    pub unsafe fn from_raw_fd(raw_fd: RawFd) -> Self {
        // safety: simple wrapper around already unsafe code
        unsafe { OwnedHandle::Fd(std::os::fd::OwnedFd::from_raw_fd(raw_fd)) }
    }

    #[cfg(target_os = "windows")]
    /// Creates a `OwnedHandle` from a RawHandle
    ///
    /// # Safety
    /// The user must ensure that no one else holds `RawFd`,
    /// because `OwnedHandle` will close the file-descriptor on drop
    pub unsafe fn from_raw_fd(raw_fd: RawHandle) -> Self {
        OwnedHandle::Fd(std::os::windows::io::OwnedHandle::from_raw_handle(raw_fd))
    }

    pub fn from_socket(socket: Socket) -> Self {
        OwnedHandle::Socket(socket)
    }

    pub fn as_handle(&self) -> BorrowedHandle<'_> {
        match self {
            #[cfg(not(target_os = "windows"))]
            OwnedHandle::Fd(owned_fd) => BorrowedHandle::Fd(owned_fd.as_fd()),
            #[cfg(target_os = "windows")]
            OwnedHandle::Fd(owned_fd) => BorrowedHandle::Fd(owned_fd.as_handle()),
            OwnedHandle::Socket(socket) => BorrowedHandle::Socket(socket),
        }
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        match self {
            OwnedHandle::Fd(owned_fd) => owned_fd.try_clone().map(OwnedHandle::Fd),
            OwnedHandle::Socket(socket) => socket.try_clone().map(OwnedHandle::Socket),
        }
    }
}

pub enum OwnedHandle {
    #[cfg(target_os = "windows")]
    Fd(std::os::windows::io::OwnedHandle),
    #[cfg(not(target_os = "windows"))]
    Fd(std::os::fd::OwnedFd),
    Socket(Socket),
}

#[derive(Copy, Clone)]
pub enum BorrowedHandle<'a> {
    #[cfg(target_os = "windows")]
    Fd(std::os::windows::io::BorrowedHandle<'a>),
    #[cfg(not(target_os = "windows"))]
    Fd(std::os::fd::BorrowedFd<'a>),
    Socket(&'a Socket),
}

#[cfg(not(target_os = "windows"))]
impl std::os::fd::AsRawFd for BorrowedHandle<'_> {
    fn as_raw_fd(&self) -> std::os::unix::prelude::RawFd {
        use std::os::fd::AsFd;
        match self {
            BorrowedHandle::Fd(fd) => fd.as_raw_fd(),
            BorrowedHandle::Socket(sock_ref) => sock_ref.as_fd().as_raw_fd(),
        }
    }
}

#[cfg(not(target_os = "windows"))]
impl std::os::fd::AsFd for BorrowedHandle<'_> {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        match self {
            BorrowedHandle::Fd(fd) => *fd,
            BorrowedHandle::Socket(sock_ref) => sock_ref.as_fd(),
        }
    }
}

#[cfg(not(target_os = "windows"))]
impl std::os::fd::AsFd for OwnedHandle {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        match self {
            OwnedHandle::Fd(fd) => fd.as_fd(),
            OwnedHandle::Socket(sock_ref) => sock_ref.as_fd(),
        }
    }
}

pub trait AsRawOsHandle {
    #[cfg(not(target_os = "windows"))]
    fn as_raw_os_handle(&self) -> std::os::fd::RawFd;
    #[cfg(target_os = "windows")]
    fn as_raw_os_handle(&self) -> std::os::windows::io::RawSocket;
}

#[cfg(not(target_os = "windows"))]
impl AsRawOsHandle for &'_ Socket {
    fn as_raw_os_handle(&self) -> std::os::fd::RawFd {
        self.as_fd().as_raw_fd()
    }
}

#[cfg(target_os = "windows")]
impl AsRawOsHandle for &'_ Socket {
    fn as_raw_os_handle(&self) -> std::os::windows::io::RawSocket {
        self.as_socket().as_raw_socket()
    }
}

#[cfg(target_os = "windows")]
pub fn pread<T: AsRawHandle>(
    fd: &T,
    buf: &mut [u8],
    offset: Option<u64>,
) -> Result<usize, std::io::Error> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::ReadFile;
    use windows_sys::Win32::System::IO::OVERLAPPED;

    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let mut bytes_read: u32 = 0;

    if let Some(offset) = offset {
        // Split offset into high and low parts
        overlapped.Anonymous.Anonymous.OffsetHigh = ((offset >> 32) & 0xFFFFFFFF) as u32;
        overlapped.Anonymous.Anonymous.Offset = (offset & 0xFFFFFFFF) as u32;
    }

    let handle = fd.as_raw_handle() as HANDLE;

    let success = unsafe {
        ReadFile(
            handle,
            buf.as_mut_ptr() as *mut _,
            buf.len() as u32,
            &mut bytes_read,
            if offset.is_some() {
                &mut overlapped
            } else {
                std::ptr::null_mut()
            },
        )
    };

    if success == 0 {
        // Error handling would go here
        return Err(std::io::Error::last_os_error());
    }

    Ok(bytes_read as usize)
}

#[cfg(target_os = "windows")]
pub fn pwrite<T: AsRawHandle>(
    fd: &T,
    buf: &[u8],
    offset: Option<u64>,
) -> Result<usize, std::io::Error> {
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::Storage::FileSystem::WriteFile;
    use windows_sys::Win32::System::IO::OVERLAPPED;

    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let mut bytes_written: u32 = 0;
    if let Some(offset) = offset {
        // Split offset into high and low parts
        overlapped.Anonymous.Anonymous.OffsetHigh = ((offset >> 32) & 0xFFFFFFFF) as u32;
        overlapped.Anonymous.Anonymous.Offset = (offset & 0xFFFFFFFF) as u32;
    }

    let handle = fd.as_raw_handle() as HANDLE;

    let success = unsafe {
        WriteFile(
            handle,
            buf.as_ptr() as *const _,
            buf.len() as u32,
            &mut bytes_written,
            if offset.is_some() {
                &mut overlapped
            } else {
                std::ptr::null_mut()
            },
        )
    };

    if success == 0 {
        return Err(std::io::Error::last_os_error());
    }

    Ok(bytes_written as usize)
}

#[cfg(not(target_os = "windows"))]
pub fn os_clock() -> Clock {
    use std::time::Duration;

    use rustix::time::ClockId;

    Clock::new(Duration::new(0, 1), || {
        let timespec = rustix::time::clock_gettime(ClockId::Monotonic);
        let sec = timespec.tv_sec as u64;
        let nano = timespec.tv_nsec as u64;
        sec * 1_000_000_000 + nano
    })
}

#[cfg(target_os = "windows")]
pub fn os_clock() -> Clock {
    use std::time::Duration;
    let mut freq = 0i64;
    let res =
        unsafe { windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut freq) };
    if res == 0 {
        panic!("failed to get freq {res:?}");
    }
    let duration = Duration::from_nanos(1_000_000_000u64 / freq as u64);
    Clock::new(duration, || {
        let mut count = 0i64;
        if unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut count) }
            == 0
        {
            panic!("failed to get count");
        };
        count as u64
    })
}
