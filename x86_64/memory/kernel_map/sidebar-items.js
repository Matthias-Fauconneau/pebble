initSidebarItems({"constant":[["BOOT_INFO",""],["HEAP_END",""],["HEAP_START","This is the address of the start of the kernel heap. The heap is 100 KiB."],["KERNEL_BASE","This is the base of the kernel address space. It starts at -2GB. We don't know how much memory the kernel image will take up when loaded into memory, so we leave quite a lot of space until the next statically mapped thing."],["KERNEL_P4_ENTRY","The kernel is mapped into the 511th entry of the P4."],["KERNEL_P4_START","This address can be used to access the kernel page table's P4 table all the time. It does not make use of the recursive mapping, so can be used when we're modifying another set of tables by installing them into the kernel's recursive entry. This mapping is set up by the bootloader."],["LOCAL_APIC_CONFIG","The virtual address that the configuration page of the local APIC is mapped to. We don't manage this using a simple `PhysicalMapping` because we need to be able to access the local APIC from interrupt handlers, which can't easily access owned `PhysicalMapping`s."],["P4_TABLE_RECURSIVE_ADDRESS","This address can be used to access the currently mapped P4 table, assuming the correct entry is recursively mapped properly."],["PHYSICAL_MAPPING_END",""],["PHYSICAL_MAPPING_START","This is the address of the start of the area in the kernel address space for random physical mappings. We reserve 32 frames."],["RECURSIVE_ENTRY","We use the 510th entry of the PML4 (P4) to access the page tables easily using the recursive paging trick. Any address that would use this entry can therefore not be used. This entry was picked because it places the unusable portion of the virtual address space between the userspace and kernel portions, which is less inconvienient than it being a hole."]]});