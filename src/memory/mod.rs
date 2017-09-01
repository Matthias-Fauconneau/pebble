/*
 * Copyright (C) 2017, Isaac Woods.
 * See LICENCE.md
 */

mod area_frame_allocator;
mod paging;

pub use self::paging::test_paging;

pub use self::area_frame_allocator::AreaFrameAllocator;
use self::paging::PhysicalAddress;

pub const PAGE_SIZE : usize = 4096;

#[derive(Debug,PartialEq,Eq,PartialOrd,Ord)]
pub struct Frame
{
    number : usize
}

impl Frame
{
    fn get_containing_frame(address : usize) -> Frame
    {
        Frame { number : address / PAGE_SIZE }
    }

    fn get_start_address(&self) -> PhysicalAddress
    {
        self.number * PAGE_SIZE
    }
}

pub trait FrameAllocator
{
    fn allocate_frame(&mut self) -> Option<Frame>;
    fn deallocate_frame(&mut self, frame : Frame);
}
