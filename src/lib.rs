use nvn_macro::*;
use libc::*;

static mut DEVICE_HAS_INIT: bool = false;
static mut GLOBAL_DEVICE: Device = Device::new();

pub use nn::vi::NativeWindowHandle as NativeWindowHandle;

#[repr(C)]
pub struct CommandHandle(u64);

#[repr(C)]
pub struct TextureHandle(u64);

#[repr(C)]
pub struct ImageHandle(u64);

extern "C" {
    fn nvnBootstrapLoader(ident: *const c_char) -> *const c_void;
}

fn nvn_resolver(ident: &str) -> (*const c_void, bool) {
    unsafe {
        if !DEVICE_HAS_INIT {
            if !nvn_internal_nvnDeviceGetProcAddress_func_ptr.is_null() {
                (nvnDeviceGetProcAddress(0 as _, ident.as_ptr() as _), false)
            } else {
                (nvnBootstrapLoader(ident.as_ptr() as _), false)
            }
        } else {
            if ident == "nvnDeviceGetProcAddress\0" { // prevent infinite recursion
                if nvn_internal_nvnDeviceGetProcAddress_func_ptr.is_null() {
                    nvn_internal_nvnDeviceGetProcAddress_func_ptr = nvnBootstrapLoader(ident.as_ptr() as _) as _;
                }
                let ret = std::mem::transmute::<_, extern "C" fn(*const Device, *const c_char) -> *const c_void>(nvn_internal_nvnDeviceGetProcAddress_func_ptr)(&GLOBAL_DEVICE, ident.as_ptr() as _);
                (ret, true)
            } else {
                (GLOBAL_DEVICE.get_proc(ident.as_ptr() as _), true)
            }
        }
    }
}

pub fn init() {
    unsafe {
        nvnDeviceInitialize::resolve();
        nvnDeviceGetProcAddress::resolve();
        nvn_internal_nvnDeviceGetProcAddress_is_resolved = true;
        nvn_internal_nvnDeviceInitialize_is_resolved = true;
        let mut builder = DeviceBuilder::new();
        builder.set_defaults();
        builder.set_flags(0);
        DEVICE_HAS_INIT = GLOBAL_DEVICE.init(&builder);
        nvn_internal_nvnDeviceGetProcAddress_is_resolved = false;
        nvn_internal_nvnDeviceInitialize_is_resolved = false;
        if DEVICE_HAS_INIT {
            DeviceBuilder::resolve();
            Device::resolve();
        }
    }
}

pub fn global_device() -> &'static mut Device {
    unsafe { &mut GLOBAL_DEVICE }
}

#[nvn_struct(0x40, nvn_resolver)]
pub struct DeviceBuilder {
    #[nvn_proc(fn nvnDeviceBuilderSetDefaults())]
    pub set_defaults: (),
    #[nvn_proc(fn nvnDeviceBuilderSetFlags(flags: u32))]
    pub set_flags: ()
}

#[nvn_struct(0x40, nvn_resolver)]
pub struct QueueBuilder {
    #[nvn_proc(fn nvnQueueBuilderSetDevice(device: *const Device))]
    pub set_device: (),
    #[nvn_proc(fn nvnQueueBuilderSetDefaults())]
    pub set_defaults: (),
    #[nvn_proc(fn nvnQueueBuilderSetFlags(flags: u32))]
    pub set_flags: (),
    #[nvn_proc(fn nvnQueueBuilderSetCommandMemorySize(size: usize))]
    pub set_command_mem_size: (),
    #[nvn_proc(fn nvnQueueBuilderSetComputeMemorySize(size: usize))]
    pub set_compute_mem_size: (),
    #[nvn_proc(const fn nvnQueueBuilderGetQueueMemorySize() -> usize)]
    pub get_mem_size: (),
    #[nvn_proc(fn nvnQueueBuilderSetQueueMemorySize(size: usize))]
    pub set_mem_size: (),
    #[nvn_proc(fn nvnQueueBuilderSetCommandFlushThreshold(size: usize))]
    pub set_cmd_flush_threshold: ()
}

#[nvn_struct(0x3000, nvn_resolver)]
pub struct Device {
    #[nvn_proc(fn nvnDeviceInitialize(builder: *const DeviceBuilder) -> bool)]
    pub init: (),
    #[nvn_proc(fn nvnDeviceFinalize())]
    pub fini: (),
    #[nvn_proc(const fn nvnDeviceGetProcAddress(ident: *const c_char) -> *const c_void)]
    pub get_proc: (),
    #[nvn_proc(fn nvnDeviceSetDebugLabel(label: *const c_char))]
    pub set_name: (),
    #[nvn_proc(const fn nvnDeviceGetInteger(what: u32, out: *mut i32))]
    pub get_int: (),
    #[nvn_proc(const fn nvnDeviceGetCurrentTimestampInNanoseconds())]
    pub get_time_nanos: (),
    #[nvn_proc(const fn nvnDeviceGetTextureHandle(texture_id: i32, sampler_id: i32) -> TextureHandle)]
    pub get_texture_handle: (),
    #[nvn_proc(const fn nvnDeviceGetTexelFetchHandle(texture_id: i32) -> TextureHandle)]
    pub get_texel_handle: (),
    #[nvn_proc(const fn nvnDeviceGetImageHandle(image_id: i32) -> ImageHandle)]
    pub get_image_handle: ()
}

#[nvn_struct(0x2000, nvn_resolver)]
pub struct Queue {
    #[nvn_proc(fn nvnQueueInitialize(builder: *const QueueBuilder) -> bool)]
    pub init: (),
    #[nvn_proc(fn nvnQueueFinalize())]
    pub fini: (),
    #[nvn_proc(fn nvnQueueSubmitCommands(count: i32, handles: *const CommandHandle))]
    pub submit_commands: (),
    #[nvn_proc(fn nvnQueueFlush())]
    pub flush: (),
    
}