pub mod address_space;
pub mod memory_object;
pub mod task;

use core::sync::atomic::{AtomicU64, Ordering};

/// Each kernel object is assigned a unique 64-bit ID, which is never reused. An ID of `0` is never allocated, and
/// is used as a sentinel value.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct KernelObjectId(u64);

/// A kernel object ID of `0` is reserved as a sentinel value that will never point to a real kernel object. It is
/// used to mark things like the `owner` of a kernel object being the kernel itself.
pub const SENTINEL_KERNEL_ID: KernelObjectId = KernelObjectId(0);

/// The next available `KernelObjectId`. It is shared between all the CPUs, and so is incremented atomically.
static KERNEL_OBJECT_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn alloc_kernel_object_id() -> KernelObjectId {
    // TODO: I think this can be Ordering::Relaxed?
    // TODO: this wraps, so we should manually detect when it wraps around and panic to prevent ID reuse
    KernelObjectId(KERNEL_OBJECT_ID_COUNTER.fetch_add(1, Ordering::SeqCst))
}

// TODO: we could use the `downcast` crate to downcast trait objects into their real types (I think)?
/// This trait should be implemented by all types that implement kernel objects, and allows common code to
/// be generic over all kernel objects. Kernel objects are generally handled as `Arc<T>` where `T` is the type
/// implementing `KernelObject`, and so interior mutability should be used for data that needs to be mutable within
/// the kernel object.
pub trait KernelObject {
    fn id(&self) -> KernelObjectId;
}

// This doesn't really work because hygiene opt-out (needed for the fields) still isn't implemented :(
// macro kernel_object {
//     ($(#[$outer_meta:meta])*
//     struct $name:ident {
//         $($(#[$field_meta:meta])*$vis:vis $field:ident: $type:ty),*$(,)?
//     }
// ) => {
//     $(#[$outer_meta])*
//     pub struct $name {
//         pub id: $crate::object::KernelObjectId,
//         pub owner: $crate::object::KernelObjectId,
//         $($(#[$field_meta])* $vis $field: $type),*
//     }

//     impl $crate::object::KernelObject for $name {
//         fn id(&self) -> $crate::object::KernelObjectId {
//             self.id
//         }
//     }
// }
// }
