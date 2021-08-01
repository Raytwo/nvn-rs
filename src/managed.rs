use super::*;

use thiserror::Error;

use std::fmt;
use std::ops::{Deref, DerefMut, Range};
use std::sync::atomic::{AtomicUsize, Ordering};
pub struct InitializeError(&'static str);

impl fmt::Debug for InitializeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Failed to initialize {} object!", self.0)
    }
}

macro_rules! impl_init_err {
    ($ty:ty) => {
        impl $ty {
            const fn get_err() -> InitializeError {
                InitializeError(stringify!($ty))
            }
        }
    }
}

pub struct MemPoolBuilder {
    builder: MemoryPoolBuilder,
    shared: bool
}

impl MemPoolBuilder {
    pub fn new() -> Self {
        MemPoolBuilder{
            builder: {
                let mut ret = MemoryPoolBuilder::default();
                ret.set_defaults();
                ret
            },
            shared: false
        }
    }

    pub fn with_device(mut self, device: *const Device) -> Self {
        self.set_device(device);
        self
    }

    pub fn with_flags(mut self, flags: MemoryPoolFlags) -> Self {
        self.set_flags(flags);
        self
    }

    pub fn with_storage(mut self, memory: *const u8, size: usize) -> Self {
        if !self.get_memory().is_null() && !self.shared {
            unsafe {
                libc::free(self.get_memory() as *mut c_void);
            }
        }
        self.set_storage(memory, size);
        self.shared = false;
        self
    }

    pub fn with_shared_storage(mut self, memory: *const u8, size: usize) -> Self {
        if !self.get_memory().is_null() && !self.shared {
            unsafe {
                libc::free(self.get_memory() as *mut c_void);
            }
        }
        self.set_storage(memory, size);
        self.shared = true;
        self
    }

    #[track_caller]
    pub fn make_storage(self, size: usize, align: Option<usize>) -> Self {
        let memory = if let Some(align) = align {
            unsafe {
                libc::memalign(align, size)
            }
        } else {
            unsafe {
                libc::memalign(0x1000, size)
            }
        };

        if memory.is_null() {
            panic!("MemPoolBuilder unable to allocate storage");
        }

        self.with_storage(memory as *const u8, size)
    }

    pub fn create(&self) -> Result<MemPool, InitializeError> {
        MemPool::create(self)
    }

    pub fn finish(self) -> Result<MemPool, InitializeError> {
        MemPool::create(&self)
    }
}

impl Deref for MemPoolBuilder {
    type Target = MemoryPoolBuilder;

    fn deref(&self) -> &Self::Target {
        &self.builder
    }
}

impl DerefMut for MemPoolBuilder {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.builder
    }
}

pub struct MemPool {
    pool: MemoryPool,
    offset: AtomicUsize,
    shared: bool,
    memory: *mut u8
}

#[derive(Error, Debug)]
pub enum MemPoolError {
    #[error("Memory is not accessible to the CPU")]
    NoCPUAccess,
    #[error("Out of memory")]
    OutOfMemory,
    #[error("Memory is null")]
    NullMemory,
}

pub struct GpuMemory<'a> {
    pool: &'a MemPool,
    range: Range<usize>
}

impl<'a> GpuMemory<'a> {
    pub fn cpu(&self) -> Result<&'a [u8], MemPoolError> {
        if self.pool.get_flags().cpu_no_access() {
            Err(MemPoolError::NoCPUAccess)
        } else {
            let mem = self.pool.map();
            if mem.is_null() {
                Err(MemPoolError::NullMemory)
            } else {
                Ok(unsafe {
                    self.cpu_unchecked()
                })
            }
        }
    }

    pub unsafe fn cpu_unchecked(&self) -> &'a [u8] {
        std::slice::from_raw_parts(self.pool.map(), self.pool.get_size())
    }

    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    pub fn into_range(self) -> Range<usize> {
        self.range.clone()
    }
}

impl<'a> Drop for GpuMemory<'a> {
    fn drop(&mut self) {
        self.pool.flush(self.range.start, self.range.end)
    }
}

impl_init_err!(MemPool);

impl MemPool {
    pub fn new() -> MemPoolBuilder {
        MemPoolBuilder::new()
    }

    pub fn create(builder: &MemPoolBuilder) -> Result<Self, InitializeError> {
        let mut pool = MemoryPool::default();
        if pool.initialize(builder.deref()) {
            Ok(Self {
                pool,
                offset: AtomicUsize::new(0),
                shared: builder.shared,
                memory: builder.get_memory()
            })
        } else {
            Err(Self::get_err())
        }
    }

    pub fn reserve_mem(&mut self, size: usize) -> Result<GpuMemory, MemPoolError> {
        if self.offset.load(Ordering::SeqCst) + size > self.get_size() {
            Err(MemPoolError::OutOfMemory)
        } else {
            let start = self.offset.fetch_add(size, Ordering::SeqCst);
            Ok(GpuMemory {
                pool: self,
                range: (start..start + size)
            })
        }
    }

    pub fn as_ref(&self) -> &MemoryPool {
        &self.pool
    }

    pub fn as_mut(&mut self) -> &mut MemoryPool {
        &mut self.pool
    }
}

impl Deref for MemPool {
    type Target = MemoryPool;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl DerefMut for MemPool {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

impl Drop for MemPool {
    fn drop(&mut self) {
        self.finalize();
        if !self.shared {
            unsafe {
                libc::free(self.memory as *mut c_void);
            }
        }
    }
}

pub struct CommandBufferBuilder {
    device: *mut Device,
    control: (*mut u8, usize),
    shared: bool,
    pool: *mut MemoryPool,
    command: (usize, usize)
}

impl CommandBufferBuilder {
    pub fn new() -> Self {
        Self {
            device: 0 as _,
            control: (0 as _, 0),
            shared: false,
            pool: 0 as _,
            command: (0, 0)
        }
    }

    pub fn with_device(mut self, device: *mut Device) -> Self {
        self.device = device;
        self
    }

    pub fn with_control(mut self, memory: *mut u8, size: usize) -> Self {
        if !self.control.0.is_null() && !self.shared {
            unsafe {
                libc::free(self.control.0 as *mut c_void);
            }
        }
        self.control = (memory, size);
        self.shared = false;
        self
    }

    pub fn with_shared_control(mut self, memory: *mut u8, size: usize) -> Self {
        if !self.control.0.is_null() && !self.shared {
            unsafe {
                libc::free(self.control.0 as *mut c_void);
            }
        }
        self.control = (memory, size);
        self.shared = true;
        self
    }

    #[track_caller]
    pub fn make_control(self, size: usize, align: Option<usize>) -> Self {
        if !self.control.0.is_null() && !self.shared {
            unsafe {
                libc::free(self.control.0 as *mut c_void);
            }
        }

        let control = if let Some(align) = align {
            unsafe {
                libc::memalign(align, size)
            }
        } else {
            unsafe {
                libc::memalign(0x1000, size)
            }
        };

        if control.is_null() {
            panic!("CommandBufferBuilder unable to allocate storage!");
        }

        self.with_control(control as *mut u8, size)
    }

    pub fn with_command(mut self, memory_pool: *mut MemoryPool, start: usize, size: usize) -> Self {
        self.pool = memory_pool;
        self.command = (start, size);
        self
    }

    pub fn finish(self) -> Result<CommandBuffer, InitializeError> {
        CommandBuffer::create(self)
    }
}

impl Drop for CommandBufferBuilder {
    fn drop(&mut self) {
        if !self.shared {
            unsafe {
                libc::free(self.control.0 as *mut libc::c_void);
            }
        }
    }
}

pub struct CommandBuffer {
    buffer: super::CommandBuffer,
    control: (*mut u8, usize),
    shared: bool,
    command: (usize, usize),
    pool: *mut MemoryPool
}

impl_init_err!(CommandBuffer);

impl CommandBuffer {
    pub fn new() -> CommandBufferBuilder {
        CommandBufferBuilder::new()
    }

    pub fn create(builder: CommandBufferBuilder) -> Result<Self, InitializeError> {
        let CommandBufferBuilder { device, control, shared, pool, command } = builder;
        let mut buffer = super::CommandBuffer::new();
        if buffer.initialize(device) {
            let mut ret = Self {
                buffer,
                control,
                shared,
                command,
                pool
            };

            ret.reset();

            Ok(ret)
        } else {
            Err(Self::get_err())
        }
    }

    pub fn reset(&mut self) {
        let command_pool = self.pool;
        let command_start = self.command.0 as u64;
        let command_size = self.command.1;
        self.add_command_memory(command_pool, command_start, command_size);
        let control_start = self.control.0;
        let control_size = self.control.1;
        self.add_control_memory(control_start, control_size);
    }

    pub fn as_ref(&self) -> &super::CommandBuffer {
        &self.buffer
    }

    pub fn as_mut(&mut self) -> &mut super::CommandBuffer {
        &mut self.buffer
    }
}

impl Deref for CommandBuffer {
    type Target = super::CommandBuffer;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl DerefMut for CommandBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

impl Drop for CommandBuffer {
    fn drop(&mut self) {
        if !self.shared {
            unsafe {
                libc::free(self.control.0 as *mut libc::c_void);
            }
        }
    }
}

unsafe impl Send for MemPool {}
unsafe impl Sync for MemPool {}
unsafe impl Send for CommandBuffer {}
unsafe impl Sync for CommandBuffer {}