#![allow(dead_code)]
use alloc::vec::Vec;

use crate::config::PAGE_SIZE;

// Execution of programs
pub const AT_NULL: usize = 0; /* end of vector */
pub const AT_IGNORE: usize = 1; /* entry should be ignored */
pub const AT_EXECFD: usize = 2; /* file descriptor of program */
pub const AT_PHDR: usize = 3; /* program headers for program */
pub const AT_PHENT: usize = 4; /* size of program header entry */
pub const AT_PHNUM: usize = 5; /* number of program headers */
pub const AT_PAGESZ: usize = 6; /* system page size */
pub const AT_BASE: usize = 7; /* base address of interpreter */
pub const AT_FLAGS: usize = 8; /* flags */
pub const AT_ENTRY: usize = 9; /* entry point of program */
pub const AT_NOTELF: usize = 10; /* program is not ELF */
pub const AT_UID: usize = 11; /* real uid */
pub const AT_EUID: usize = 12; /* effective uid */
pub const AT_GID: usize = 13; /* real gid */
pub const AT_EGID: usize = 14; /* effective gid */
pub const AT_PLATFORM: usize = 15; /* string identifying CPU for optimizations */
pub const AT_HWCAP: usize = 16; /* arch dependent hints at CPU capabilities */
pub const AT_CLKTCK: usize = 17; /* frequency at which times() increments */
/* AT_* values 18 through 22 are reserved */
pub const AT_SECURE: usize = 23; /* secure mode boolean */
pub const AT_BASE_PLATFORM: usize = 24; /* string identifying real platform, may
                                         * differ from AT_PLATFORM. */
pub const AT_RANDOM: usize = 25; /* address of 16 random bytes */
pub const AT_HWCAP2: usize = 26; /* extension of AT_HWCAP */

pub const AT_EXECFN: usize = 31; /* filename of program */
/* Pointer to the global system page used for system calls and other
nice things.  */
pub const AT_SYSINFO: usize = 32;
pub const AT_SYSINFO_EHDR: usize = 33;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AuxHeader {
    pub aux_type: usize,
    pub value: usize,
}

impl AuxHeader {
    pub fn generate(ph_entry_size: usize, ph_count: usize, entry_point: usize) -> Vec<Self> {
        let mut auxv = Vec::new();

        macro_rules! push {
            ($x1: expr, $x2: expr) => {
                auxv.push(AuxHeader {
                    aux_type: $x1,
                    value: $x2,
                });
            };
        }
        push!(AT_PHENT, ph_entry_size);
        push!(AT_PHNUM, ph_count);
        push!(AT_PAGESZ, PAGE_SIZE);
        push!(AT_BASE, 0);
        push!(AT_FLAGS, 0);
        push!(AT_ENTRY, entry_point);
        push!(AT_NOTELF, 0x112d);
        push!(AT_UID, 0);
        push!(AT_EUID, 0);
        push!(AT_GID, 0);
        push!(AT_EGID, 0);
        push!(AT_PLATFORM, 0);
        push!(AT_HWCAP, 0);
        push!(AT_CLKTCK, 100);
        push!(AT_SECURE, 0);
        auxv
    }
    pub fn reverse() -> usize {
        40 * 8 * 2
    }
    pub fn write_to(self, dst: &mut [usize; 2]) {
        *dst = [self.aux_type, self.value];
    }
}
