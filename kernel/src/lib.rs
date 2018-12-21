#![cfg_attr(not(test), no_std)]
#![feature(
    asm,
    decl_macro,
    allocator_api,
    const_fn,
    alloc,
    alloc_error_handler,
    core_intrinsics
)]
extern crate alloc;

/*
 * This selects the correct module to include depending on the architecture we're compiling the
 * kernel for. These architecture modules contain the kernel entry point and any platform-specific
 * code.
 */
cfg_if! {
    if #[cfg(feature = "arch_x86_64")] {
        mod x86_64;
        pub use crate::x86_64::kmain;
    } else {
        compile_error!("Tried to build kernel without specifying an architecture!");
    }
}

mod heap_allocator;
mod util;

use crate::heap_allocator::LockedHoleAllocator;
use cfg_if::cfg_if;
use core::panic::PanicInfo;
use log::error;

#[cfg(not(test))]
#[global_allocator]
pub static ALLOCATOR: LockedHoleAllocator = LockedHoleAllocator::new_uninitialized();

#[cfg(not(test))]
#[panic_handler]
#[no_mangle]
fn panic(info: &PanicInfo) -> ! {
    error!("KERNEL PANIC: {}", info);
    loop {
        // TODO: arch-independent cpu halt?
    }
}
