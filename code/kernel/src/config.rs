#![allow(unused)]

pub const USER_STACK_SIZE: usize = 4096 * 2; // 4096 * 2
pub const KERNEL_STACK_SIZE: usize = 4096 * 2; // 4096 * 2
pub const KERNEL_HEAP_SIZE: usize = 0x2_0000; // 128KB
pub const PAGE_SIZE: usize = 0x1000; // 0x1000
pub const PAGE_SIZE_BITS: usize = 12; // 12

pub const TRAMPOLINE: usize = 0xffff_ffff_ffff_f000;

// 1GB
pub const HARDWARD_BEGIN: usize = 0xffff_ffff_c000_0000;
pub const HARDWARD_END: usize = 0xffff_ffff_ffff_f000;

// 8MB
/// only used in init pagetable, then need to replace to range MEMORY
pub const INIT_MEMORY_SIZE: usize = 0x80_0000; // 8MB = 2^23
pub const INIT_MEMORY_END: usize = KERNEL_TEXT_BEGIN + INIT_MEMORY_SIZE;

// 1GB
pub const KERNEL_TEXT_BEGIN: usize = 0xffff_ffff_8000_0000;
pub const KERNEL_TEXT_END: usize = 0xffff_ffff_c000_0000;

/// 32GB
///
/// MEMORY_BEGIN mapping to 0x0
///
/// KERNEL_TEXT_BEGIN in 0xffff_fff0_8000_0000 need to mapping in entry.asm
pub const MEMORY_BEGIN: usize = 0xffff_fff0_0000_0000;
pub const MEMORY_END: usize = 0xffff_fff8_0000_0000;
pub const MEMORY_SIZE: usize = MEMORY_END - MEMORY_BEGIN;
/// change a kernel text pointer to direct memory pointer by minus this.
///
/// ptr(kernel text) = ptr(direct memory) + this
pub const MEMORY_KERNEL_OFFSET: usize = (KERNEL_TEXT_BEGIN - PHYSICAL_OFFSET) - MEMORY_BEGIN;
/// eliminate init memory, previous space had been used.
pub const MEMORY_INIT_KERNEL_END: usize = INIT_MEMORY_END - MEMORY_KERNEL_OFFSET;

// 64GB
pub const IOMAP_BEGIN: usize = 0xffff_ffd0_0000_0000;
pub const IOMAP_END: usize = 0xffff_ffe0_0000_0000;

// total range: 256GB
pub const KERNEL_BASE: usize = 0xffff_ffc0_0000_0000;

pub const PHYSICAL_OFFSET: usize = 0x8000_0000;
pub const PHYSICAL_MEMORY_OFFSET: usize = MEMORY_BEGIN;
