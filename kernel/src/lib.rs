//! This module probably looks rather sparse! Check the root of one of the architecture modules for an entry point.
#![cfg_attr(not(test), no_std)]
#![allow(incomplete_features)]
#![feature(
    asm, decl_macro, allocator_api, const_fn, alloc_error_handler, core_intrinsics, trait_alias, type_ascription, naked_functions, box_syntax, const_generics, global_asm
)]
#[macro_use] extern crate alloc;

mod heap_allocator;
mod object;
// mod per_cpu;
// mod scheduler;
// mod syscall;

use crate::heap_allocator::LockedHoleAllocator;
use cfg_if::cfg_if;
use core::panic::PanicInfo;
use hal::{boot_info::BootInfo, Hal};
use libpebble::syscall::system_object::FramebufferSystemObjectInfo;
use log::{error, info};

cfg_if! {
    if #[cfg(feature = "arch_x86_64")] {
        type HalImpl = hal_x86_64::HalImpl;
    } else {
        compile_error!("No architecture supplied, or target arch does not have a HAL implementation configured!");
    }
}

#[cfg(not(test))]
#[global_allocator]
pub static ALLOCATOR: LockedHoleAllocator = LockedHoleAllocator::new_uninitialized();

#[no_mangle]
pub extern "C" fn kmain(boot_info: &BootInfo) -> ! {
    HalImpl::init_logger();
    info!("The Pebble kernel is running");

    if boot_info.magic != hal::boot_info::BOOT_INFO_MAGIC {
        panic!("Boot info magic is not correct!");
    }

    /*
     * Initialise the heap allocator. After this, the kernel is free to use collections etc. that
     * can allocate on the heap through the global allocator.
     */
    unsafe {
        #[cfg(not(test))]
        ALLOCATOR.lock().init(boot_info.heap_address, boot_info.heap_size);
    }

    let hal = HalImpl::new(boot_info);

    // TODO: start doing stuff
    loop {}
}

#[cfg(not(test))]
#[panic_handler]
#[no_mangle]
fn panic(info: &PanicInfo) -> ! {
    error!("KERNEL PANIC: {}", info);
    loop {
        // TODO: arch-independent cpu halt?
    }
}
