//! This module defines the kernel entry-point on x86_64.

mod acpi_handler;
mod address_space;
mod cpu;
mod interrupts;
mod logger;
mod memory;
mod memory_object;
mod per_cpu;
mod task;

// Export the items that every architecture module is expected to provide to the rest of the
// kernel.
pub use self::{
    per_cpu::{common_per_cpu_data, common_per_cpu_data_mut},
    task::context_switch,
};

use self::{
    acpi_handler::PebbleAcpiHandler,
    address_space::AddressSpace,
    interrupts::InterruptController,
    logger::KernelLogger,
    memory::LockedPhysicalMemoryManager,
    memory_object::MemoryObject,
    task::Task,
};
use crate::{
    arch::Architecture,
    object::{KernelObject, WrappedKernelObject},
    scheduler::Scheduler,
    x86_64::per_cpu::per_cpu_data_mut,
};
use aml::AmlContext;
use core::time::Duration;
use log::{error, info, warn};
use pebble_util::InitGuard;
use spin::{Mutex, RwLock};
use x86_64::{
    boot::{BootInfo, ImageInfo},
    hw::{cpu::CpuInfo, gdt::Gdt, registers::read_control_reg},
    memory::{kernel_map, Frame, PageTable, PhysicalAddress},
};

pub(self) static GDT: Mutex<Gdt> = Mutex::new(Gdt::new());

pub(self) static ARCH: InitGuard<Arch> = InitGuard::uninit();

pub struct Arch {
    pub cpu_info: CpuInfo,
    pub physical_memory_manager: LockedPhysicalMemoryManager,
    /// Each bit in this bitmap corresponds to a slot for an address space worth of kernel stacks
    /// in the kernel address space. We can have up 1024 address spaces, so need 128 bytes.
    pub kernel_stack_bitmap: Mutex<[u8; 128]>,
    pub kernel_page_table: Mutex<PageTable>,
}

/// `Arch` contains a bunch of things, like the GDT, that the hardware relies on actually being at
/// the memory addresses we say they're at. We can stop them moving using `Unpin`, but can't stop
/// them from being dropped, so we just panic if the architecture struct is dropped.
impl Drop for Arch {
    fn drop(&mut self) {
        panic!("The `Arch` has been dropped. This should never happen!");
    }
}

impl Architecture for Arch {
    type AddressSpace = AddressSpace;
    type Task = Task;
    type MemoryObject = MemoryObject;

    fn drop_to_userspace(&self, task: WrappedKernelObject<Arch>) -> ! {
        task::drop_to_usermode(task);
    }
}

/// This is the entry point for the kernel on x86_64. It is called from the UEFI bootloader and
/// initialises the system, then passes control into the common part of the kernel.
#[no_mangle]
pub fn kmain() -> ! {
    /*
     * Initialise the logger.
     */
    log::set_logger(&KernelLogger).unwrap();
    log::set_max_level(log::LevelFilter::Trace);
    info!("The Pebble kernel is running");

    let cpu_info = CpuInfo::new();
    info!(
        "We're running on an {:?} processor, model info = {:?}, microarch = {:?}",
        cpu_info.vendor,
        cpu_info.model_info,
        cpu_info.microarch()
    );
    check_support(&cpu_info);

    /*
     * Initialise the heap allocator. After this, the kernel is free to use collections etc. that
     * can allocate on the heap through the global allocator.
     */
    #[cfg(not(test))]
    unsafe {
        crate::ALLOCATOR.lock().init(kernel_map::HEAP_START, kernel_map::HEAP_END);
    }

    /*
     * Retrieve the `BootInfo` passed to us from the bootloader and make sure it has the correct
     * magic number.
     */
    let boot_info = unsafe { &mut *(kernel_map::BOOT_INFO.mut_ptr::<BootInfo>()) };
    if boot_info.magic != x86_64::boot::BOOT_INFO_MAGIC {
        panic!("Boot info magic number is not correct!");
    }

    /*
     * Parse the static ACPI tables.
     */
    let acpi_info = match boot_info.rsdp_address {
        Some(rsdp_address) => {
            let mut handler = PebbleAcpiHandler;
            match acpi::parse_rsdp(&mut handler, usize::from(rsdp_address)) {
                Ok(acpi_info) => Some(acpi_info),

                Err(err) => {
                    error!("Failed to parse static ACPI tables: {:?}", err);
                    warn!("Continuing. Some functionality may not work, or the kernel may panic!");
                    None
                }
            }
        }

        None => None,
    };

    info!("{:#?}", acpi_info);

    /*
     * Register all the CPUs we can find.
     */
    // let (mut boot_processor, application_processors) = match acpi_info {
    //     Some(ref info) => {
    //         assert!(
    //             info.boot_processor.is_some()
    //                 && info.boot_processor.unwrap().state == ProcessorState::Running
    //         );
    //         // TODO: Cpu shouldn't manage the TSS anymore - that should be the job of the per-cpu
    //         // data
    //         let tss = Tss::new();
    //         let tss_selector = unsafe { GDT.lock().add_tss(TssSegment::new(&tss)) };
    //         let boot_processor = Cpu::from_acpi(&info.boot_processor.unwrap(), tss, tss_selector);

    //         let mut application_processors = Vec::new();
    //         for application_processor in &info.application_processors {
    //             if application_processor.state == ProcessorState::Disabled {
    //                 continue;
    //             }

    //             let tss = Tss::new();
    //             let tss_selector = unsafe { GDT.lock().add_tss(TssSegment::new(&tss)) };
    //             application_processors.push(Cpu::from_acpi(&application_processor, tss, tss_selector));
    //         }

    //         (boot_processor, application_processors)
    //     }

    //     None => {
    //         /*
    //          * We couldn't find the number of processors from the ACPI tables. Just create a TSS
    //          * for this one.
    //          */
    //         let tss = Tss::new();
    //         let tss_selector = unsafe { GDT.lock().add_tss(TssSegment::new(Pin::new(&tss))) };
    //         let cpu = Cpu { processor_uid: 0, local_apic_id: 0, is_ap: false, tss, tss_selector };
    //         (cpu, Vec::with_capacity(0))
    //     }
    // };

    /*
     * Set up the main kernel data structure, which also initializes the physical memory manager.
     * From this point, we can freely allocate physical memory from any point in the kernel.
     *
     * This assumes that the bootloader has correctly installed a set of page tables, including a
     * full physical mapping in the correct location. Strange things will happen if this is not
     * true, so this process is a tad unsafe.
     */
    ARCH.initialize(Arch {
        cpu_info,
        physical_memory_manager: LockedPhysicalMemoryManager::new(boot_info),
        kernel_page_table: Mutex::new(unsafe {
            PageTable::from_frame(
                Frame::starts_with(PhysicalAddress::new(read_control_reg!(cr3) as usize).unwrap()),
                kernel_map::PHYSICAL_MAPPING_BASE,
            )
        }),
        kernel_stack_bitmap: Mutex::new([0; 128]),
    });

    /*
     * Initialize the common kernel data structures too.
     */
    crate::COMMON.initialize(crate::Common::new());

    /*
     * Create the per-cpu data, then load the GDT, then install the per-cpu data. This has to be
     * done in this specific order because loading the GDT after setting GS_BASE will override it.
     */
    let (guarded_per_cpu, tss_selector) = per_cpu::GuardedPerCpu::new();
    unsafe {
        // TODO: having to lock it prevents `load` from taking a pinned reference, reference with
        // 'static, which we should probably deal with.
        GDT.lock().load(tss_selector);
    }
    guarded_per_cpu.install();

    // TODO: deal gracefully with a bad ACPI parse
    // TODO: maybe don't take arch here and instead access it through COMMON
    let mut interrupt_controller = InterruptController::init(
        &ARCH.get(),
        match acpi_info {
            Some(ref info) => info.interrupt_model.as_ref().unwrap(),
            None => unimplemented!(),
        },
    );
    interrupt_controller.enable_local_timer(&ARCH.get(), Duration::from_secs(3));

    /*
     * Parse the DSDT.
     */
    let mut aml_context = AmlContext::new();
    if let Some(dsdt_info) = acpi_info.and_then(|info| info.dsdt) {
        let virtual_address =
            kernel_map::physical_to_virtual(PhysicalAddress::new(dsdt_info.address).unwrap());
        info!(
            "DSDT parse: {:?}",
            aml_context.parse_table(unsafe {
                core::slice::from_raw_parts(virtual_address.ptr(), dsdt_info.length as usize)
            })
        );
    }

    /*
     * Create the backup framebuffer if the bootloader switched to a graphics mode.
     */
    if let Some(ref video_info) = boot_info.video_info {
        create_framebuffer(video_info);
    }

    /*
     * Load all the images as initial tasks, and add them to the scheduler's ready list.
     */
    let mut scheduler = &mut unsafe { per_cpu_data_mut() }.common_mut().scheduler;
    info!("Adding {} initial tasks to the ready queue", boot_info.num_images);
    for image in boot_info.images() {
        load_task(&ARCH.get(), scheduler, image);
    }

    info!("Dropping to usermode");
    scheduler.drop_to_userspace(&ARCH.get())
}

fn create_framebuffer(video_info: &x86_64::boot::VideoInfo) {
    use x86_64::memory::{EntryFlags, FrameSize, Size4KiB, VirtualAddress};

    /*
     * For now, we just put the framebuffer at the start of the region where we map MemoryObjects
     * into userspace address spaces. We might run into issues with this in the future.
     */
    const VIRTUAL_ADDRESS: VirtualAddress = self::memory::userspace_map::MEMORY_OBJECTS_START;
    /*
     * We only support RGB32 and BGR32 pixel formats, so there will always be 4 bytes per pixel.
     */
    const BPP: u32 = 4;

    let size_in_bytes = (video_info.stride * video_info.height * BPP) as usize;
    let memory_object = KernelObject::MemoryObject(RwLock::new(box MemoryObject::new(
        VIRTUAL_ADDRESS,
        video_info.framebuffer_address,
        pebble_util::math::align_up(size_in_bytes, Size4KiB::SIZE) / Size4KiB::SIZE,
        EntryFlags::PRESENT | EntryFlags::WRITABLE | EntryFlags::USER_ACCESSIBLE | EntryFlags::NO_CACHE,
    )))
    .add_to_map(&mut crate::COMMON.get().object_map.write());

    *crate::COMMON.get().backup_framebuffer_object.lock() = Some(memory_object.id);
}

fn load_task(arch: &Arch, scheduler: &mut Scheduler, image: &ImageInfo) {
    let object_map = &mut crate::COMMON.get().object_map.write();

    // Make an AddressSpace for the image
    let address_space: WrappedKernelObject<Arch> =
        KernelObject::AddressSpace(RwLock::new(box AddressSpace::new(&arch))).add_to_map(object_map);

    // Make a MemoryObject for each segment and map it into the AddressSpace
    for segment in image.segments() {
        let memory_object =
            KernelObject::MemoryObject(RwLock::new(box MemoryObject::from_boot_info(&segment)))
                .add_to_map(object_map);
        address_space
            .object
            .address_space()
            .unwrap()
            .write()
            .map_memory_object(memory_object: WrappedKernelObject<Arch>);
    }

    // Create a Task for the image and add it to the scheduler's ready queue
    let task = KernelObject::Task(RwLock::new(
        box Task::from_image_info(&arch, address_space.clone(), image).unwrap(),
    ))
    .add_to_map(object_map);
    scheduler.add_task(task).unwrap();
}

/// We rely on certain processor features to be present for simplicity and sanity-retention. This
/// function checks that we support everything we need to, and enable features that need to be.
fn check_support(cpu_info: &CpuInfo) {
    use bit_field::BitField;
    use x86_64::hw::registers::{read_control_reg, write_control_reg, CR4_XSAVE_ENABLE_BIT};

    if !cpu_info.supported_features.xsave {
        panic!("Processor does not support xsave instruction!");
    }

    let mut cr4 = read_control_reg!(CR4);
    cr4.set_bit(CR4_XSAVE_ENABLE_BIT, true);
    unsafe {
        write_control_reg!(CR4, cr4);
    }
}
