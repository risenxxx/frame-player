//! The Vulkan render API of our own libmpv (`render_vk.h`, a downstream draft; the patch is in
//! `src-skathi/patches/`) as the producer, in place of route A's OpenGL. Added to the first
//! producer: the overlay target
//! (`MPV_RENDER_PARAM_VULKAN_OVERLAY_FBO`) and the Metal un-premultiply pass for it.
//!
//! The app owns a MoltenVK device (through the Vulkan loader, so that the validation layers can
//! be put in front of it), and a ring of `'RGhA'` IOSurfaces, each imported into Vulkan with
//! `VkImportMetalIOSurfaceInfoEXT` and given a timeline semaphore of its own, exported as an
//! `MTLSharedEvent`. One timeline per slot carries both directions:
//!
//! ```text
//!   value v      the slot is free (Skathi's copy of the previous frame is done)
//!   mpv:         acquire = v, release (ready) = v + 1      (GPU wait, GPU signal)
//!   Skathi:      present_shared_fenced(wait = v + 1, release = v + 2)
//!   refused:     nothing was submitted; the next acquire is v + 1 itself
//! ```
//!
//! Nothing waits on the CPU in the frame loop: mpv's GPU work waits for Skathi's copy, Skathi's
//! copy waits for mpv's frame. The CPU waits only when a slot is destroyed or read back.

use std::ffi::{c_char, c_int, c_void, CStr};
use std::time::Duration;

use ash::vk;
use skathi::SharedTexture;

use crate::sys::IoSurface;

pub const PARAM_VULKAN_INIT_PARAMS: c_int = 0x7601;
pub const PARAM_VULKAN_FBO: c_int = 0x7602;
pub const PARAM_VULKAN_RETIRE: c_int = 0x7603;
pub const PARAM_VULKAN_OVERLAY_FBO: c_int = 0x7604;
/// `picture_layout`: the picture image is the whole element, scaled (our patch's addition).
pub const PICTURE_ELEMENT: c_int = 1;

/// `mpv_vulkan_overlay_fbo` (render_vk.h, our patch): 128 bytes.
#[repr(C)]
pub struct OverlayFbo {
    pub struct_size: usize,
    pub target: Fbo,
    pub rendered: c_int,
    pub video_rect: [c_int; 4],
    pub source_rect: [c_int; 4],
    pub picture_layout: c_int,
}

extern "C" {
    fn fpui_unpremul_new(metallib: *const c_void, len: usize) -> *mut c_void;
    fn fpui_unpremul_texture(ctx: *mut c_void, surface: *mut c_void, w: c_int, h: c_int, eight: c_int) -> *mut c_void;
    #[allow(clippy::too_many_arguments)]
    fn fpui_unpremul_run(ctx: *mut c_void, texture: *mut c_void, dest: *mut c_void, event: *mut c_void, wait: u64, signal: u64, offset_y: u32, mode: u32, scan: c_int) -> c_int;
    fn fpui_unpremul_box(ctx: *mut c_void, scans: *mut u64) -> u64;
    fn fpui_unpremul_free(ctx: *mut c_void);
    fn fpui_release(obj: *mut c_void);
}

/// The Metal pass that turns the overlay's premultiplied pixels into the straight alpha Skathi
/// composites (`unpremul.m`).
pub struct Unpremul(*mut c_void, bool);
unsafe impl Send for Unpremul {}
/// The Metal context goes with the render thread that made it. It was never released: `drop(unpremul)` at the
/// end of `render_thread_child` did nothing while this type had no `Drop` (clippy's `drop_non_drop`, 1.100).
impl Drop for Unpremul {
    fn drop(&mut self) {
        unsafe { fpui_unpremul_free(self.0) };
    }
}
impl Unpremul {
    /// `linear`: mpv's overlay holds linear values (target-trc=linear), so an 8-bit output is
    /// sRGB-encoded here; with an sRGB target it is not.
    pub fn new(linear: bool) -> Result<Self, String> {
        // FPUI_METALLIB=1 loads the kernels precompiled at build time.
        static LIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/unpremul.metallib"));
        let pre = std::env::var("FPUI_METALLIB").is_ok_and(|v| v == "1") && !LIB.is_empty();
        let p = unsafe { if pre { fpui_unpremul_new(LIB.as_ptr().cast(), LIB.len()) } else { fpui_unpremul_new(std::ptr::null(), 0) } };
        if p.is_null() { Err("the Metal un-premultiply pass did not build".into()) } else { Ok(Self(p, linear)) }
    }
    /// Whether an 8-bit output holds linear values, set again when the picture's
    /// format follows the file.
    pub fn set_linear(&mut self, linear: bool) {
        self.1 = linear;
    }
    /// The drawn rows (top, bottom) the last completed scan found, None if nothing was
    /// drawn or no scan has completed; and the count of completed scans.
    pub fn last_box(&self) -> (Option<(u32, u32)>, u64) {
        let mut n = 0u64;
        let b = unsafe { fpui_unpremul_box(self.0, &mut n) };
        (if b == u64::MAX { None } else { Some(((b >> 32) as u32, b as u32)) }, n)
    }
    fn texture(&self, slot: &mut Slot) -> bool {
        if slot.mtl_texture.is_null() {
            slot.mtl_texture = unsafe { fpui_unpremul_texture(self.0, slot.surface.0, slot.size.0 as c_int, slot.size.1 as c_int, slot.eight as c_int) };
        }
        !slot.mtl_texture.is_null()
    }
    /// GPU: wait for `wait` on the slot's timeline, un-premultiply (an 8-bit slot holds linear
    /// values: also sRGB-encode), signal `signal`.
    pub fn run(&self, slot: &mut Slot, wait: u64, signal: u64, scan: bool) -> bool {
        if !self.texture(slot) {
            return false;
        }
        unsafe { fpui_unpremul_run(self.0, slot.mtl_texture, std::ptr::null_mut(), slot.event, wait, signal, 0, (slot.eight && self.1) as u32, scan as c_int) == 0 }
    }
    /// The band: rows `offset_y..` of `slot` into `band`, un-premultiplied, on the
    /// slot's timeline.
    pub fn run_band(&self, slot: &mut Slot, band: &mut Band, offset_y: u32, wait: u64, signal: u64, scan: bool) -> bool {
        if !self.texture(slot) {
            return false;
        }
        if band.mtl_texture.is_null() {
            band.mtl_texture = unsafe { fpui_unpremul_texture(self.0, band.surface.0, band.size.0 as c_int, band.size.1 as c_int, band.eight as c_int) };
            if band.mtl_texture.is_null() {
                return false;
            }
        }
        // Mode bit 0: an 8-bit output is sRGB-encoded (the source holds linear values either way).
        unsafe { fpui_unpremul_run(self.0, slot.mtl_texture, band.mtl_texture, slot.event, wait, signal, offset_y, (band.eight && self.1) as u32, scan as c_int) == 0 }
    }
}

/// A cropped copy of an overlay slot: its own IOSurface, opened in Skathi on
/// the source slot's timeline. Output 8-bit when `eight`.
pub struct Band {
    pub surface: IoSurface,
    pub shared: SharedTexture,
    mtl_texture: *mut c_void,
    pub size: (u32, u32),
    pub eight: bool,
}
unsafe impl Send for Band {}
impl Band {
    pub fn new(src: &Slot, width: u32, height: u32, eight: bool) -> Result<Self, String> {
        let surface = if eight { IoSurface::bgra(width, height) } else { IoSurface::rgba_half(width, height) };
        // SAFETY: the surface and the event outlive the texture (bands are dropped before slots).
        let shared = unsafe { SharedTexture::from_iosurface(surface.0).and_then(|t| t.with_shared_event(src.event)) }
            .map_err(|e| format!("opening the band in Skathi: {e}"))?;
        Ok(Self { surface, shared, mtl_texture: std::ptr::null_mut(), size: (width, height), eight })
    }
}
impl Drop for Band {
    fn drop(&mut self) {
        if !self.mtl_texture.is_null() {
            unsafe { fpui_release(self.mtl_texture) };
        }
    }
}

/// `mpv_vulkan_init_params` (render_vk.h). 104 bytes in C.
#[repr(C)]
pub struct InitParams {
    pub struct_size: usize,
    pub instance: vk::Instance,
    pub get_instance_proc_addr: vk::PFN_vkGetInstanceProcAddr,
    pub physical_device: vk::PhysicalDevice,
    pub device: vk::Device,
    pub api_version: u32,
    pub extensions: *const *const c_char,
    pub num_extensions: c_int,
    pub features: *const vk::PhysicalDeviceFeatures2<'static>,
    pub queue_family_index: u32,
    pub queue_count: u32,
    pub lock_queue: Option<extern "C" fn(*mut c_void, u32, u32)>,
    pub unlock_queue: Option<extern "C" fn(*mut c_void, u32, u32)>,
    pub queue_ctx: *mut c_void,
}

/// `mpv_vulkan_fbo` (render_vk.h). 80 bytes in C.
#[repr(C)]
pub struct Fbo {
    pub struct_size: usize,
    pub image: vk::Image,
    pub w: c_int,
    pub h: c_int,
    pub format: vk::Format,
    pub usage: vk::ImageUsageFlags,
    pub layout: vk::ImageLayout,
    pub final_layout: vk::ImageLayout,
    pub queue_family: u32,
    pub acquire_semaphore: vk::Semaphore,
    pub acquire_value: u64,
    pub release_semaphore: vk::Semaphore,
    pub release_value: u64,
}

/// The C sizes and offsets, measured with the header; a mismatch would
/// hand mpv garbage, so it is checked before anything is created.
pub fn check_layout() -> Result<(), String> {
    let init = std::mem::size_of::<InitParams>();
    let fbo = std::mem::size_of::<Fbo>();
    let off_final = std::mem::offset_of!(Fbo, final_layout);
    let off_acq = std::mem::offset_of!(Fbo, acquire_semaphore);
    let off_relv = std::mem::offset_of!(Fbo, release_value);
    let off_feat = std::mem::offset_of!(InitParams, features);
    let off_ctx = std::mem::offset_of!(InitParams, queue_ctx);
    let ovl = (std::mem::size_of::<OverlayFbo>(), std::mem::offset_of!(OverlayFbo, rendered), std::mem::offset_of!(OverlayFbo, picture_layout));
    if ovl != (128, 88, 124) {
        return Err(format!("mpv_vulkan_overlay_fbo layout differs: {ovl:?}"));
    }
    if (init, fbo, off_final, off_acq, off_relv, off_feat, off_ctx) != (104, 80, 36, 48, 72, 64, 96) {
        return Err(format!(
            "struct layout differs from render_vk.h: init {init} fbo {fbo} final_layout@{off_final} \
             acquire_semaphore@{off_acq} release_value@{off_relv} features@{off_feat} queue_ctx@{off_ctx}"
        ));
    }
    Ok(())
}

/// The device mpv borrows. Every pointer handed to mpv lives in a `Box` here, until after
/// `mpv_render_context_free`. (`entry` keeps the loader loaded.)
#[allow(dead_code)]
pub struct VkEnv {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub physical: vk::PhysicalDevice,
    pub device: ash::Device,
    pub family: u32,
    metal: ash::ext::metal_objects::Device,
    memory_types: vk::PhysicalDeviceMemoryProperties,
    /// Vulkan holds a pointer to the strings' array, the Vec's heap buffer, which stays put when the Vec moves.
    extensions: Vec<*const c_char>,
    _sync2: Box<vk::PhysicalDeviceSynchronization2Features<'static>>,
    _f12: Box<vk::PhysicalDeviceVulkan12Features<'static>>,
    features: Box<vk::PhysicalDeviceFeatures2<'static>>,
    pub init: Box<InitParams>,
    pub description: String,
}

unsafe impl Send for VkEnv {}

/// The Vulkan loader and MoltenVK the app ships, never a system or Homebrew one: an installed copy's
/// `Contents/Frameworks/libvulkan.1.dylib` with its ICD file in `Contents/Resources/vulkan/icd.d`,
/// a development build's `src-skathi/lib` (`scripts/build-macos-libs.sh --v2`). Each only when the
/// variable is not set already, so `MP_VK_LIB` (another library, e.g. a MoltenVK dylib loaded
/// directly) and `VK_DRIVER_FILES` still override. Sets the environment, so it runs before any
/// thread starts.
pub fn default_env() {
    let (lib, icd) = match crate::commands::bundle_contents() {
        Some(c) => (c.join("Frameworks/libvulkan.1.dylib"), c.join("Resources/vulkan/icd.d/MoltenVK_icd.json")),
        None => {
            let lib = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("lib");
            (lib.join("libvulkan.1.dylib"), lib.join("MoltenVK_icd.json"))
        }
    };
    if std::env::var_os("MP_VK_LIB").is_none() {
        std::env::set_var("MP_VK_LIB", &lib);
    }
    // The loader also searches the system's ICD folders; naming ours keeps any other MoltenVK out.
    if std::env::var_os("VK_DRIVER_FILES").is_none() && std::env::var_os("VK_ICD_FILENAMES").is_none() {
        std::env::set_var("VK_DRIVER_FILES", &icd);
    }
}

/// The Vulkan library to load: `MP_VK_LIB`, which [`default_env`] sets to the app's own loader.
pub fn library() -> String {
    std::env::var("MP_VK_LIB").unwrap_or_else(|_| "libvulkan.1.dylib".into())
}

impl VkEnv {
    pub fn new() -> Result<Self, String> {
        check_layout()?;
        let lib = library();
        unsafe {
            let entry = ash::Entry::load_from(&lib).map_err(|e| format!("loading {lib}: {e}"))?;
            let available = entry.enumerate_instance_extension_properties(None).unwrap_or_default();
            let has_i = |name: &CStr| available.iter().any(|e| CStr::from_ptr(e.extension_name.as_ptr()) == name);
            let mut iext = Vec::new();
            let mut flags = vk::InstanceCreateFlags::empty();
            if has_i(ash::khr::portability_enumeration::NAME) {
                iext.push(ash::khr::portability_enumeration::NAME.as_ptr());
                flags |= vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
            }
            let app = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_2);
            // vkExportMetalObjectsEXT for the MTLDevice needs this on the instance.
            let mut export_device = vk::ExportMetalObjectCreateInfoEXT::default()
                .export_object_type(vk::ExportMetalObjectTypeFlagsEXT::METAL_DEVICE);
            let instance = entry
                .create_instance(
                    &vk::InstanceCreateInfo::default()
                        .flags(flags)
                        .application_info(&app)
                        .enabled_extension_names(&iext)
                        .push_next(&mut export_device),
                    None,
                )
                .map_err(|e| format!("vkCreateInstance: {e}"))?;
            let physical = *instance
                .enumerate_physical_devices()
                .map_err(|e| format!("enumerating devices: {e}"))?
                .first()
                .ok_or("no Vulkan device")?;
            let mut driver = vk::PhysicalDeviceDriverProperties::default();
            let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut driver);
            instance.get_physical_device_properties2(physical, &mut props2);
            let description = format!(
                "{} / {} {}",
                CStr::from_ptr(props2.properties.device_name.as_ptr()).to_string_lossy(),
                CStr::from_ptr(driver.driver_name.as_ptr()).to_string_lossy(),
                CStr::from_ptr(driver.driver_info.as_ptr()).to_string_lossy()
            );
            let family = instance
                .get_physical_device_queue_family_properties(physical)
                .iter()
                .position(|f| f.queue_flags.contains(vk::QueueFlags::GRAPHICS | vk::QueueFlags::COMPUTE))
                .ok_or("no graphics + compute queue")? as u32;
            let available = instance.enumerate_device_extension_properties(physical).map_err(|e| e.to_string())?;
            let has = |name: &CStr| available.iter().any(|e| CStr::from_ptr(e.extension_name.as_ptr()) == name);
            let mut extensions = vec![ash::ext::metal_objects::NAME.as_ptr()];
            if !has(ash::ext::metal_objects::NAME) {
                return Err("VK_EXT_metal_objects is missing".into());
            }
            if has(ash::khr::portability_subset::NAME) {
                extensions.push(ash::khr::portability_subset::NAME.as_ptr());
            }
            // Without synchronization2, libplacebo's legacy barriers carry srcStageMask = 0, which
            // the validation layers reject.
            if !has(ash::khr::synchronization2::NAME) {
                return Err("VK_KHR_synchronization2 is missing".into());
            }
            extensions.push(ash::khr::synchronization2::NAME.as_ptr());
            // The feature chain mpv is told about must be the one the device was made with.
            let mut sync2 = Box::new(vk::PhysicalDeviceSynchronization2Features::default().synchronization2(true));
            let mut f12 = Box::new(
                vk::PhysicalDeviceVulkan12Features::default()
                    .timeline_semaphore(true)
                    .host_query_reset(true),
            );
            f12.p_next = (&mut *sync2 as *mut vk::PhysicalDeviceSynchronization2Features).cast();
            let mut features = Box::new(vk::PhysicalDeviceFeatures2::default());
            features.p_next = (&mut *f12 as *mut vk::PhysicalDeviceVulkan12Features).cast();
            let priorities = [1.0f32];
            let queues = [vk::DeviceQueueCreateInfo::default()
                .queue_family_index(family)
                .queue_priorities(&priorities)];
            let mut info = vk::DeviceCreateInfo::default()
                .queue_create_infos(&queues)
                .enabled_extension_names(&extensions);
            info.p_next = (&*features as *const vk::PhysicalDeviceFeatures2).cast();
            let device = instance
                .create_device(physical, &info, None)
                .map_err(|e| format!("vkCreateDevice: {e}"))?;
            let init = Box::new(InitParams {
                struct_size: std::mem::size_of::<InitParams>(),
                instance: instance.handle(),
                get_instance_proc_addr: entry.static_fn().get_instance_proc_addr,
                physical_device: physical,
                device: device.handle(),
                api_version: vk::API_VERSION_1_2,
                extensions: extensions.as_ptr(),
                num_extensions: extensions.len() as c_int,
                features: &*features,
                queue_family_index: family,
                queue_count: 1,
                lock_queue: None,
                unlock_queue: None,
                queue_ctx: std::ptr::null_mut(),
            });
            Ok(Self {
                metal: ash::ext::metal_objects::Device::new(&instance, &device),
                memory_types: instance.get_physical_device_memory_properties(physical),
                entry,
                instance,
                physical,
                device,
                family,
                extensions,
                _sync2: sync2,
                _f12: f12,
                features,
                init,
                description,
            })
        }
    }

    pub fn init_params(&mut self) -> *mut c_void {
        (&mut *self.init as *mut InitParams).cast()
    }

    /// `eight`: a `'BGRA'` / `B8G8R8A8_UNORM` slot, else `'RGhA'` / half float.
    pub fn slot_fmt(&self, width: u32, height: u32, skathi: bool, eight: bool) -> Result<Slot, String> {
        let surface = if eight { IoSurface::bgra(width, height) } else { IoSurface::rgba_half(width, height) };
        self.slot_on(surface, width, height, skathi, eight)
    }

    /// A slot on an IOSurface somebody else made (a CAMetalLayer drawable's). The
    /// surface is retained here and released with the slot.
    pub fn slot_ext(&self, iosurface: *mut c_void, width: u32, height: u32, eight: bool) -> Result<Slot, String> {
        extern "C" {
            fn CFRetain(cf: *const c_void) -> *const c_void;
        }
        unsafe { CFRetain(iosurface) };
        self.slot_on(IoSurface(iosurface), width, height, false, eight)
    }

    fn slot_on(&self, surface: IoSurface, width: u32, height: u32, skathi: bool, eight: bool) -> Result<Slot, String> {
        let d = &self.device;
        // A 10-bit drawable (child.rs, an HDR layer on the PQ path) is imported as what it is.
        let format = if crate::child::iosurface_format(surface.0) == "l10r" {
            vk::Format::A2R10G10B10_UNORM_PACK32
        } else if eight {
            vk::Format::B8G8R8A8_UNORM
        } else {
            vk::Format::R16G16B16A16_SFLOAT
        };
        unsafe {
            let mut import = vk::ImportMetalIOSurfaceInfoEXT::default().io_surface(surface.0 as vk::IOSurfaceRef);
            let usage = vk::ImageUsageFlags::COLOR_ATTACHMENT
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST
                | vk::ImageUsageFlags::SAMPLED;
            let image = d
                .create_image(
                    &vk::ImageCreateInfo::default()
                        .image_type(vk::ImageType::TYPE_2D)
                        .format(format)
                        .extent(vk::Extent3D { width, height, depth: 1 })
                        .mip_levels(1)
                        .array_layers(1)
                        .samples(vk::SampleCountFlags::TYPE_1)
                        .tiling(vk::ImageTiling::OPTIMAL)
                        .usage(usage)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .initial_layout(vk::ImageLayout::UNDEFINED)
                        .push_next(&mut import),
                    None,
                )
                .map_err(|e| format!("vkCreateImage: {e}"))?;
            let need = d.get_image_memory_requirements(image);
            let memory_type = (0..self.memory_types.memory_type_count)
                .find(|i| {
                    need.memory_type_bits & (1 << i) != 0
                        && self.memory_types.memory_types[*i as usize]
                            .property_flags
                            .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
                })
                .ok_or("no memory type for the image")?;
            let memory = d
                .allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(need.size)
                        .memory_type_index(memory_type),
                    None,
                )
                .map_err(|e| format!("vkAllocateMemory: {e}"))?;
            d.bind_image_memory(image, memory, 0).map_err(|e| format!("vkBindImageMemory: {e}"))?;
            let mut timeline = vk::SemaphoreTypeCreateInfo::default()
                .semaphore_type(vk::SemaphoreType::TIMELINE)
                .initial_value(0);
            let mut export = vk::ExportMetalObjectCreateInfoEXT::default()
                .export_object_type(vk::ExportMetalObjectTypeFlagsEXT::METAL_SHARED_EVENT);
            let semaphore = d
                .create_semaphore(
                    &vk::SemaphoreCreateInfo::default().push_next(&mut timeline).push_next(&mut export),
                    None,
                )
                .map_err(|e| format!("vkCreateSemaphore: {e}"))?;
            let mut event = vk::ExportMetalSharedEventInfoEXT::default().semaphore(semaphore);
            let mut objects = vk::ExportMetalObjectsInfoEXT::default().push_next(&mut event);
            (self.metal.fp().export_metal_objects_ext)(self.metal.device(), &mut objects);
            if event.mtl_shared_event.is_null() {
                return Err("the semaphore exported no MTLSharedEvent".into());
            }
            let shared = if skathi {
                // `NoDevice` until the window has drawn once: the caller retries, so a refusal
                // must not leak the Vulkan objects made above (found by the validation layers).
                match SharedTexture::from_iosurface(surface.0)
                    .and_then(|t| t.with_shared_event(event.mtl_shared_event))
                {
                    Ok(t) => Some(t),
                    Err(e) => {
                        d.destroy_semaphore(semaphore, None);
                        d.destroy_image(image, None);
                        d.free_memory(memory, None);
                        return Err(format!("opening the IOSurface in Skathi: {e}"));
                    }
                }
            } else {
                None
            };
            Ok(Slot {
                event: event.mtl_shared_event,
                mtl_texture: std::ptr::null_mut(),
                surface,
                image,
                memory,
                usage,
                semaphore,
                shared,
                value: 0,
                used: false,
                size: (width, height),
                format,
                eight,
            })
        }
    }

    /// A bounded CPU wait on the slot's timeline.
    pub fn wait(&self, slot: &Slot, value: u64, timeout: Duration) -> bool {
        let semaphores = [slot.semaphore];
        let values = [value];
        unsafe {
            self.device
                .wait_semaphores(
                    &vk::SemaphoreWaitInfo::default().semaphores(&semaphores).values(&values),
                    timeout.as_nanos() as u64,
                )
                .is_ok()
        }
    }

    /// Frees a slot once mpv has retired it: waits (bounded) for the last value anybody will
    /// signal (Skathi's release of the last accepted frame), then destroys.
    pub fn destroy(&self, slot: Slot) -> bool {
        let done = slot.value == 0 || self.wait(&slot, slot.value, Duration::from_secs(2));
        unsafe {
            self.device.destroy_image(slot.image, None);
            self.device.free_memory(slot.memory, None);
            self.device.destroy_semaphore(slot.semaphore, None);
        }
        if !slot.mtl_texture.is_null() {
            unsafe { fpui_release(slot.mtl_texture) };
        }
        drop(slot.shared);
        drop(slot.surface);
        done
    }
}

impl Drop for VkEnv {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_device(None);
            self.instance.destroy_instance(None);
        }
        let _ = (&self.extensions, &self.features);
    }
}

pub struct Slot {
    /// The timeline's MTLSharedEvent (owned by the semaphore).
    event: *mut c_void,
    /// The overlay pass's Metal view of the IOSurface, made on first use.
    mtl_texture: *mut c_void,
    pub surface: IoSurface,
    pub image: vk::Image,
    memory: vk::DeviceMemory,
    usage: vk::ImageUsageFlags,
    semaphore: vk::Semaphore,
    pub shared: Option<SharedTexture>,
    /// The value mpv's next acquire waits for: Skathi's release of the last accepted frame, or
    /// mpv's own ready value when that frame was refused.
    pub value: u64,
    /// Contents and layout are defined (GENERAL) once mpv has rendered into it.
    pub used: bool,
    pub size: (u32, u32),
    format: vk::Format,
    pub eight: bool,
}

impl Slot {
    /// The timeline's MTLSharedEvent.
    pub fn event(&self) -> *mut c_void {
        self.event
    }

    /// The target for one render call: acquire the current value, release the next.
    pub fn fbo(&self) -> Fbo {
        Fbo {
            struct_size: std::mem::size_of::<Fbo>(),
            image: self.image,
            w: self.size.0 as c_int,
            h: self.size.1 as c_int,
            format: self.format,
            usage: self.usage,
            layout: if self.used { vk::ImageLayout::GENERAL } else { vk::ImageLayout::UNDEFINED },
            final_layout: vk::ImageLayout::GENERAL,
            queue_family: vk::QUEUE_FAMILY_IGNORED,
            acquire_semaphore: self.semaphore,
            acquire_value: self.value,
            release_semaphore: self.semaphore,
            release_value: self.value + 1,
        }
    }
}
