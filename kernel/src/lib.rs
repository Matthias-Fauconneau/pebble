/*
 * Copyright (C) 2017, Isaac Woods.
 * See LICENCE.md
 */

#![no_std]

#![feature(alloc)]
#![feature(core_intrinsics)]

                extern crate volatile;
                extern crate spin;
                extern crate bitflags;
                extern crate bit_field;
                extern crate alloc;
#[macro_use]    extern crate log;
#[macro_use]    extern crate arch;

pub fn kernel_main()
{
    trace!("Control passed to kernel crate");
    loop { }
}
