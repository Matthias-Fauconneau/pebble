# `request_system_object`
Used by tasks to request access to a "system" kernel object - usually one created by the kernel to provide
some resource, such as the framebuffer, to userspace. Each object has a hardcoded id used to request it, and
requires the requesting task to have a particular capability - if the task is permitted access to the object,
the kernel returns the kernel object id of the object, and takes any steps needed for the requesting task to
be able to access the object. Normal user tasks probably don't have any need for this system call - it is more
aimed at device drivers and system management tasks.

If this system call is successful, access is granted to the system object from the calling task. This means it
can use the returned id in other system calls.

### Parameters
The first parameter, `a`, is always the id (not to be confused with the actual kernel object id, which is not
hardcoded and therefore can change between boots) of the system object. The meaning of the other parameters
depend on the object requested. The allowed values are:

| `a`   | Object being requested                | Type              | `b`           | `c`           | `d`           | `e`           |
|-------|---------------------------------------|-------------------|---------------|---------------|---------------|---------------|
| `0`   | The backup framebuffer                | `MemoryObject`    | ptr to info   | -             | -             | -             |

TODO: id for accessing Pci config space where extra params are bus, device, function (+segment or whatever)
numbers.

### Returns
This system call uses the standard way to fallibly return a `KernelObjectId` (detailed in the
[page on syscalls](../kernel/syscalls.md)). The status codes used are:
* `0` means that the system call was successful
* `1` means that the requested ID is valid, but that the system object hasn't been created
* `2` means that the ID does not correspond to a valid system object
* `3` means that the requested object ID is valid, but that the task does not have the correct capability
  to access it. This is returned even if the system object doesn't exist.

### Capabilities needed
| id    | Capability needed             |
|-------|-------------------------------|
| `0`   | `AccessBackupFramebuffer`     |

### System object: backup framebuffer
An ID of `0` corresponds to the backup framebuffer system object - a framebuffer created by the bootloader or
kernel that can be used if there is not a more specialized graphics driver available (e.g. on x86_64, this uses the
UEFI Graphics Output Protocol to create a basic linear framebuffer). The object is a `MemoryObject` that is meant
to be mapped into the userspace driver and directly written to as video memory.

A userspace address should be passed as `b`, which is used to pass information about the framebuffer back to
userspace from the kernel. The memory must be user-accessible and writable. The format of the written structure is:
``` rust
#[repr(C)]
struct FramebufferInfo {
    /// The address of the start of the framebuffer
    address: usize,

    width: u16,
    height: u16,
    stride: u16,
    /// 0 = RGB32
    /// 1 = BGR32
    pixel_format: u8,
}
```
