#![no_std]
#![no_main]
#![feature(panic_info_message, abi_efiapi, cell_update)]

mod allocator;
mod command_line;
mod image;
mod logger;

use allocator::BootFrameAllocator;
use command_line::CommandLine;
use core::{mem, panic::PanicInfo, slice};
use log::{error, info};
use uefi::{
    prelude::*,
    proto::{loaded_image::LoadedImage, media::fs::SimpleFileSystem},
    table::boot::{AllocateType, MemoryType, SearchType},
};
use x86_64::memory::{FrameAllocator, FrameSize, PageTable, Size4KiB, VirtualAddress};

/*
 * These are the custom UEFI memory types we use. They're all collected here so we can easily see which numbers
 * we're using.
 */
pub const KERNEL_MEMORY_TYPE: MemoryType = MemoryType::custom(0x70000000);
pub const IMAGE_MEMORY_TYPE: MemoryType = MemoryType::custom(0x70000001);
pub const PAGE_TABLE_MEMORY_TYPE: MemoryType = MemoryType::custom(0x70000002);
pub const MEMORY_MAP_MEMORY_TYPE: MemoryType = MemoryType::custom(0x70000003);

#[entry]
fn efi_main(image_handle: Handle, system_table: SystemTable<Boot>) -> Status {
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
    // TODO: return upon error instead of panicking
    let fs_handle = find_volume(&system_table, command_line.volume_label.expect("No volume label supplied"))
        .expect("No disk with the given volume label");

    /*
     * We create a set of page tables for the kernel. Because memory is identity-mapped in UEFI, we can act as
     * if we've placed the physical mapping at 0x0.
     */
    let allocator = BootFrameAllocator::new(system_table.boot_services(), 64);
    let mut page_table = PageTable::new(allocator.allocate(), VirtualAddress::new(0x0).unwrap());
    let mut mapper = page_table.mapper();

    let kernel_info = if let Some(kernel_path) = command_line.kernel_path {
        match image::load_kernel(system_table.boot_services(), fs_handle, kernel_path, &mut mapper, &allocator) {
            Ok(kernel_info) => kernel_info,
            Err(err) => {
                error!("Failed to load kernel: {:?}", err);
                return Status::LOAD_ERROR;
            }
        }
    } else {
        error!("No kernel path passed! What am I supposed to load?");
        return Status::INVALID_PARAMETER;
    };
    info!("Loaded kernel!");

    let memory_map_size = system_table.boot_services().memory_map_size();
    info!("Memory map is {} bytes long", memory_map_size);

    let pages_needed = Size4KiB::frames_needed(memory_map_size);
    let memory_map_address = system_table
        .boot_services()
        .allocate_pages(AllocateType::AnyPages, MEMORY_MAP_MEMORY_TYPE, pages_needed)
        .unwrap_success();
    let memory_map = unsafe { slice::from_raw_parts_mut(memory_map_address as *mut u8, memory_map_size) };

    logger::LOGGER.lock().disable_console_output(true);
    let system_table =
        system_table.exit_boot_services(image_handle, memory_map).expect_success("Failed to exit boot services");

    Status::SUCCESS
}

fn find_volume(system_table: &SystemTable<Boot>, label: &str) -> Option<Handle> {
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
            return Some(handle);
        }
    }

    system_table.boot_services().free_pool(pool_addr).unwrap_success();
    None
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
