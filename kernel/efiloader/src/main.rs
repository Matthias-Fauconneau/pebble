#![no_std]
#![no_main]
#![feature(panic_info_message, abi_efiapi, cell_update, asm, never_type)]

mod allocator;
mod command_line;
mod image;
mod logger;

use allocator::BootFrameAllocator;
use command_line::CommandLine;
use core::{mem, panic::PanicInfo, slice};
use hal::{
    boot_info::BootInfo,
    memory::{Flags, FrameAllocator, FrameSize, Page, PageTable, PhysicalAddress, Size4KiB, VirtualAddress},
};
use hal_x86_64::paging::PageTableImpl;
use log::{error, info};
use uefi::{
    prelude::*,
    proto::{console::gop::GraphicsOutput, loaded_image::LoadedImage, media::fs::SimpleFileSystem},
    table::boot::{AllocateType, MemoryMapIter, MemoryType, SearchType},
};

/*
 * These are the custom UEFI memory types we use. They're all collected here so we can easily see which numbers
 * we're using.
 */
pub const KERNEL_MEMORY_TYPE: MemoryType = MemoryType::custom(0x80000000);
pub const IMAGE_MEMORY_TYPE: MemoryType = MemoryType::custom(0x80000001);
pub const PAGE_TABLE_MEMORY_TYPE: MemoryType = MemoryType::custom(0x80000002);
pub const MEMORY_MAP_MEMORY_TYPE: MemoryType = MemoryType::custom(0x80000003);
pub const BOOT_INFO_MEMORY_TYPE: MemoryType = MemoryType::custom(0x80000004);
pub const KERNEL_HEAP_MEMORY_TYPE: MemoryType = MemoryType::custom(0x80000005);

#[derive(Clone, Copy, Debug)]
pub enum LoaderError {
    NoKernelPath,
    NoBootVolume,
    BootVolumeDoesNotExist,
    FailedToLoadKernel,
    FilePathDoesNotExist,
    NoValidVideoMode,
}

#[entry]
fn efi_main(image_handle: Handle, system_table: SystemTable<Boot>) -> Status {
    match main(image_handle, system_table) {
        Ok(_) => unreachable!(),
        Err(err) => {
            error!("Something went wrong: {:?}", err);
            Status::LOAD_ERROR
        }
    }
}

fn main(image_handle: Handle, system_table: SystemTable<Boot>) -> Result<!, LoaderError> {
    logger::init(system_table.stdout());
    info!("Hello, World!");

    let loaded_image_protocol = unsafe {
        &mut *system_table
            .boot_services()
            .handle_protocol::<LoadedImage>(image_handle)
            .expect_success("Failed to open LoadedImage protocol")
            .get()
    };

    const COMMAND_LINE_MAX_LENGTH: usize = 256;
    let mut buffer = [0u8; COMMAND_LINE_MAX_LENGTH];

    let load_options_str = loaded_image_protocol.load_options(&mut buffer).expect("Failed to load load options");
    let command_line = CommandLine::new(load_options_str);

    // TODO: instead of finding the volume by label, we could just grab it from the LoadedImageProtocol (I think)
    // and say they all have to be on the same volume?
    let fs_handle = find_volume(&system_table, command_line.volume_label)?;

    /*
     * We create a set of page tables for the kernel. Because memory is identity-mapped in UEFI, we can act as
     * if we've placed the physical mapping at 0x0.
     */
    let allocator = BootFrameAllocator::new(system_table.boot_services(), 64);
    let mut page_table = PageTableImpl::new(allocator.allocate(), VirtualAddress::new(0x0));

    let kernel_info = image::load_kernel(
        system_table.boot_services(),
        fs_handle,
        command_line.kernel_path?,
        &mut page_table,
        &allocator,
    )?;
    let mut next_safe_address = kernel_info.next_safe_address;

    let memory_map_size = system_table.boot_services().memory_map_size();
    let memory_map_address = system_table
        .boot_services()
        .allocate_pages(AllocateType::AnyPages, MEMORY_MAP_MEMORY_TYPE, Size4KiB::frames_needed(memory_map_size))
        .unwrap_success();
    let memory_map_buffer = unsafe { slice::from_raw_parts_mut(memory_map_address as *mut u8, memory_map_size) };

    /*
     * Construct boot info to pass to the kernel.
     */
    let boot_info_needed_frames = Size4KiB::frames_needed(mem::size_of::<BootInfo>());
    let boot_info_physical_start = system_table
        .boot_services()
        .allocate_pages(AllocateType::AnyPages, BOOT_INFO_MEMORY_TYPE, boot_info_needed_frames)
        .unwrap_success();
    let identity_boot_info_ptr = VirtualAddress::new(boot_info_physical_start as usize).mut_ptr() as *mut BootInfo;
    unsafe {
        *identity_boot_info_ptr = BootInfo::default();
    }
    let boot_info = unsafe { &mut *identity_boot_info_ptr };
    let boot_info_virtual_address = kernel_info.next_safe_address;
    next_safe_address += boot_info_needed_frames * Size4KiB::SIZE;
    page_table
        .map_area(
            boot_info_virtual_address,
            PhysicalAddress::new(boot_info_physical_start as usize).unwrap(),
            boot_info_needed_frames * Size4KiB::SIZE,
            Flags { ..Default::default() },
            &allocator,
        )
        .unwrap();
    boot_info.magic = hal::boot_info::BOOT_INFO_MAGIC;

    /*
     * Find the RSDP address and add it to the boot info.
     */
    boot_info.rsdp_address = system_table.config_table().iter().find_map(|entry| {
        use uefi::table::cfg::{ACPI2_GUID, ACPI_GUID};
        if entry.guid == ACPI_GUID || entry.guid == ACPI2_GUID {
            Some(PhysicalAddress::new(entry.address as usize).unwrap())
        } else {
            None
        }
    });

    /*
     * Switch to a suitable video mode and create a framebuffer, if the user requested us to.
     */
    if command_line.framebuffer.is_some() {
        create_framebuffer(system_table.boot_services(), boot_info, &command_line)?;
    }

    /*
     * Allocate the kernel heap.
     */
    allocate_and_map_heap(
        system_table.boot_services(),
        boot_info,
        &mut next_safe_address,
        command_line.kernel_heap_size,
        &mut page_table,
        &allocator,
    )?;

    /*
     * Load all the images we've been asked to.
     */
    for image in command_line.images() {
        let (name, path) = image.unwrap();
        info!("Loading image called '{}' from path '{}'", name, path);
        boot_info
            .loaded_images
            .add_image(image::load_image(system_table.boot_services(), fs_handle, name, path)?)
            .unwrap();
    }

    /*
     * After we've exited from the boot services, we are not able to use the ConsoleOut services, so we disable
     * printing to them in the logger.
     */
    logger::LOGGER.lock().disable_console_output(true);
    let (_system_table, memory_map) = system_table
        .exit_boot_services(image_handle, memory_map_buffer)
        .expect_success("Failed to exit boot services");
    process_memory_map(memory_map, boot_info, &mut page_table, &allocator)?;

    /*
     * Jump into the kernel!
     */
    unsafe {
        info!("Switching to new page tables");
        /*
         * We disable interrupts until the kernel has a chance to install its own IDT.
         */
        asm!("cli");
        page_table.switch_to();

        /*
         * Because we change the stack pointer, we need to load the entry point into a register, as local
         * variables will no longer be available.
         */
        info!("Jumping into kernel!\n\n\n");
        asm!("mov rsp, rax
              jmp rbx"
             :
             : "{rax}"(kernel_info.stack_top), "{rbx}"(kernel_info.entry_point), "{rdi}"(boot_info_virtual_address)
             : "rax", "rbx", "rsp", "rdi"
             : "intel"
        );
    }
    unreachable!()
}

/// Process the final UEFI memory map when after we've exited boot services. We need to do three things with it:
///     * We need to identity-map anything that UEFI expects to stay in the same place, including the loader image
///       (the code that's currently running), and the UEFI runtime services. We also map the boot services, as
///       many implementations don't actually stop using them after the call to `ExitBootServices` as they should.
///     * We construct the memory map that will be passed to the kernel, which it uses to initialise its physical
///       memory manager. This is added directly to the already-allocated boot info.
///     * Construct the physical memory mapping - we map the entirity of physical memory into the kernel address
///       space to make it easy for the kernel to access any address it needs to.
fn process_memory_map<A, P>(
    memory_map: MemoryMapIter<'_>,
    boot_info: &mut BootInfo,
    mapper: &mut P,
    allocator: &A,
) -> Result<(), LoaderError>
where
    A: FrameAllocator<Size4KiB>,
    P: PageTable<Size4KiB>,
{
    use hal::boot_info::{MemoryMapEntry, MemoryType as BootInfoMemoryType};

    /*
     * To know how much physical memory to map, we keep track of the largest physical address that appears in
     * the memory map.
     */
    let mut max_physical_address = 0x0;

    for entry in memory_map {
        /*
         * If this is the largest physical address we've seen, update it.
         */
        max_physical_address = usize::max(
            max_physical_address,
            entry.phys_start as usize + entry.page_count as usize * Size4KiB::SIZE,
        );

        /*
         * Identity-map the entry in the kernel page tables, if needed.
         */
        match entry.ty {
            MemoryType::LOADER_CODE
            | MemoryType::LOADER_DATA
            | MemoryType::BOOT_SERVICES_CODE
            | MemoryType::BOOT_SERVICES_DATA
            | MemoryType::RUNTIME_SERVICES_CODE
            | MemoryType::RUNTIME_SERVICES_DATA => {
                info!("Identity mapping area: {:?}", entry);
                /*
                 * Implementations don't appear to fill out the `VirtualStart` field, so we use the physical
                 * address for both (as we're identity-mapping anyway).
                 */
                mapper
                    .map_area(
                        VirtualAddress::new(entry.phys_start as usize),
                        PhysicalAddress::new(entry.phys_start as usize).unwrap(),
                        entry.page_count as usize * Size4KiB::SIZE,
                        Flags { writable: true, executable: true, ..Default::default() },
                        allocator,
                    )
                    .unwrap();
            }
            _ => (),
        }

        /*
         * Add the entry to the boot info memory map, if it can be used by the kernel. This memory map will only
         * be processed after we've left the loader, so we can include memory currently used by the
         * loader as free.
         */
        // TODO: move this to a decl_macro when hygiene-opt-out is implemented
        macro_rules! add_entry {
            ($type: expr) => {
                boot_info
                    .memory_map
                    .add_entry(MemoryMapEntry {
                        start: PhysicalAddress::new(entry.phys_start as usize).unwrap(),
                        size: entry.page_count as usize * Size4KiB::SIZE,
                        memory_type: $type,
                    })
                    .unwrap();
            };
        }
        match entry.ty {
            MemoryType::BOOT_SERVICES_CODE
            | MemoryType::BOOT_SERVICES_DATA
            | MemoryType::CONVENTIONAL
            | MemoryType::LOADER_CODE
            | MemoryType::LOADER_DATA
            | MEMORY_MAP_MEMORY_TYPE => add_entry!(BootInfoMemoryType::Conventional),

            MemoryType::ACPI_RECLAIM => add_entry!(BootInfoMemoryType::AcpiReclaimable),
            IMAGE_MEMORY_TYPE => add_entry!(BootInfoMemoryType::LoadedImage),
            PAGE_TABLE_MEMORY_TYPE => add_entry!(BootInfoMemoryType::KernelPageTables),
            BOOT_INFO_MEMORY_TYPE => add_entry!(BootInfoMemoryType::BootInfo),
            KERNEL_HEAP_MEMORY_TYPE => add_entry!(BootInfoMemoryType::KernelHeap),

            // Other regions will never be useable by the kernel, so we don't bother including them
            _ => (),
        }
    }

    /*
     * Construct the physical memory mapping. We find the maximum physical address that the memory map contains,
     * and map that much physical memory.
     */
    info!("Constructing physical mapping from 0x0 to {:#x}", max_physical_address);
    mapper
        .map_area(
            hal_x86_64::kernel_map::PHYSICAL_MAPPING_BASE,
            PhysicalAddress::new(0x0).unwrap(),
            max_physical_address,
            Flags { writable: true, ..Default::default() },
            allocator,
        )
        .unwrap();

    Ok(())
}

/// Allocate and map the kernel heap. This takes the current next safe virtual address, uses it for the heap, and
/// updates it.
fn allocate_and_map_heap<A, P>(
    boot_services: &BootServices,
    boot_info: &mut BootInfo,
    next_safe_address: &mut VirtualAddress,
    heap_size: usize,
    mapper: &mut P,
    allocator: &A,
) -> Result<(), LoaderError>
where
    A: FrameAllocator<Size4KiB>,
    P: PageTable<Size4KiB>,
{
    assert!(heap_size % Size4KiB::SIZE == 0);
    let frames_needed = Size4KiB::frames_needed(heap_size);
    let physical_start = boot_services
        .allocate_pages(AllocateType::AnyPages, KERNEL_HEAP_MEMORY_TYPE, frames_needed)
        .unwrap_success();

    mapper
        .map_area(
            *next_safe_address,
            PhysicalAddress::new(physical_start as usize).unwrap(),
            heap_size,
            Flags { writable: true, ..Default::default() },
            allocator,
        )
        .unwrap();

    boot_info.heap_address = *next_safe_address;
    boot_info.heap_size = heap_size;

    *next_safe_address = (Page::<Size4KiB>::contains(*next_safe_address + heap_size) + 1).start;
    Ok(())
}

fn create_framebuffer(
    boot_services: &BootServices,
    boot_info: &mut BootInfo,
    command_line: &CommandLine,
) -> Result<(), LoaderError> {
    use hal::boot_info::{PixelFormat, VideoModeInfo};
    use uefi::proto::console::gop::PixelFormat as GopFormat;

    // Make an initial call to find how many handles we need to search
    let num_handles = boot_services
        .locate_handle(SearchType::from_proto::<GraphicsOutput>(), None)
        .expect_success("Failed to get list of GOP devices");

    // Allocate a pool of the needed size
    let pool_addr = boot_services
        .allocate_pool(MemoryType::LOADER_DATA, mem::size_of::<Handle>() * num_handles)
        .expect_success("Failed to allocate pool for GOP handles");
    let handle_slice: &mut [Handle] = unsafe { slice::from_raw_parts_mut(pool_addr as *mut Handle, num_handles) };

    // Actually fetch the handles
    boot_services
        .locate_handle(SearchType::from_proto::<GraphicsOutput>(), Some(handle_slice))
        .expect_success("Failed to get list of graphics output devices");

    for &mut handle in handle_slice {
        let proto = unsafe {
            &mut *boot_services
                .handle_protocol::<GraphicsOutput>(handle)
                .expect_success("Failed to open GraphicsOutput")
                .get()
        };

        let chosen_mode = proto.modes().map(|mode| mode.unwrap()).find(|mode| {
            let (width, height) = mode.info().resolution();
            let pixel_format = mode.info().pixel_format();
            width == command_line.framebuffer.unwrap().width.unwrap() as usize
                && height == command_line.framebuffer.unwrap().height.unwrap() as usize
                && (pixel_format == GopFormat::RGB || pixel_format == GopFormat::BGR)
        });

        if let Some(mode) = chosen_mode {
            proto.set_mode(&mode).expect_success("Failed to switch to new video mode");

            let framebuffer_address = PhysicalAddress::new(proto.frame_buffer().as_mut_ptr() as usize).unwrap();
            let mode_info = mode.info();
            let (width, height) = mode_info.resolution();
            let pixel_format = match mode_info.pixel_format() {
                GopFormat::RGB => PixelFormat::RGB32,
                GopFormat::BGR => PixelFormat::BGR32,
                _ => panic!("Invalid video mode chosen!"),
            };

            boot_info.video_mode = Some(VideoModeInfo {
                framebuffer_address,
                pixel_format,
                width,
                height,
                stride: mode_info.stride(),
            });
            info!("Switched to video mode: {:?}", boot_info.video_mode);
            return Ok(());
        }
    }

    Err(LoaderError::NoValidVideoMode)
}

fn find_volume(system_table: &SystemTable<Boot>, label: &str) -> Result<Handle, LoaderError> {
    use uefi::proto::media::file::{File, FileSystemVolumeLabel};

    // Make an initial call to find how many handles we need to search
    let num_handles = system_table
        .boot_services()
        .locate_handle(SearchType::from_proto::<SimpleFileSystem>(), None)
        .expect_success("Failed to get list of filesystems");

    // Allocate a pool of the needed size
    let pool_addr = system_table
        .boot_services()
        .allocate_pool(MemoryType::LOADER_DATA, mem::size_of::<Handle>() * num_handles)
        .expect_success("Failed to allocate pool for filesystem handles");
    let handle_slice: &mut [Handle] = unsafe { slice::from_raw_parts_mut(pool_addr as *mut Handle, num_handles) };

    // Actually fetch the handles
    system_table
        .boot_services()
        .locate_handle(SearchType::from_proto::<SimpleFileSystem>(), Some(handle_slice))
        .expect_success("Failed to get list of filesystems");

    // TODO: the `&mut` here is load-bearing, because we free the pool, and so need to copy the handle for if we
    // want to return it, otherwise it disappears out from under us. This should probably be rewritten to not work
    // like that. We could use a `Pool` type that manages the allocation and is automatically freed when dropped.
    for &mut handle in handle_slice {
        let proto = unsafe {
            &mut *system_table
                .boot_services()
                .handle_protocol::<SimpleFileSystem>(handle)
                .expect_success("Failed to open SimpleFileSystem")
                .get()
        };
        let mut buffer = [0u8; 32];
        let volume_label = proto
            .open_volume()
            .expect_success("Failed to open volume")
            .get_info::<FileSystemVolumeLabel>(&mut buffer)
            .expect_success("Failed to get volume label")
            // TODO: maybe change uefi to take a buffer here and return a &str (allows us to remove dependency on
            // ucs2 here for one)
            .volume_label();

        let mut str_buffer = [0u8; 32];
        let length = ucs2::decode(volume_label.to_u16_slice(), &mut str_buffer).unwrap();
        let volume_label_str = core::str::from_utf8(&str_buffer[0..length]).unwrap();

        if volume_label_str == label {
            system_table.boot_services().free_pool(pool_addr).unwrap_success();
            return Ok(handle);
        }
    }

    system_table.boot_services().free_pool(pool_addr).unwrap_success();
    Err(LoaderError::BootVolumeDoesNotExist)
}

#[panic_handler]
fn panic_handler(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        error!("Panic in {} at ({}:{})", location.file(), location.line(), location.column());
        if let Some(message) = info.message() {
            error!("Panic message: {}", message);
        }
    }
    loop {}
}
