initSidebarItems({"mod":[["kernel_map","These constants define the layout of the memory map when the bootloader passes control to the kernel. We split the virtual address space into two regions - the kernel address space between `0xffff_ffff_8000_0000` and `0xffff_ffff_ffff_ffff`, and the userspace address space between `0x0000_0000_0000_0000` and `0xffff_efff_ffff_ffff`. These are non-contiguous because the 510th entry of the PML4 is recursively mapped so we can access the page tables."],["paging",""]],"struct":[["PhysicalAddress","Represents an address in the physical memory space. A valid physical address is smaller than 2^52"]]});