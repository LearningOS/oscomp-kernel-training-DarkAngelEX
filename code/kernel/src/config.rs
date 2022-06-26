#![allow(dead_code)]

pub const USER_STACK_SIZE: usize = PAGE_SIZE * 8; // 4096 * 2
pub const USER_STACK_RESERVE: usize = PAGE_SIZE; // 4096 * 1
pub const KERNEL_STACK_SIZE: usize = PAGE_SIZE * 8; // 4096 * 8

/// ============================== KERNEL ==============================
///
/// 0x8_0000 = 512KB
/// 0x10_0000 = 1MB
pub const KERNEL_HEAP_SIZE: usize = 0x200_0000; // 2MB

use crate::{memory::address::UserAddr, tools::range::URange};

pub const PAGE_SIZE: usize = 0x1000; // 0x1000
pub const PAGE_SIZE_BITS: usize = 12; // 12
#[deprecated]
pub const TRAMPOLINE: usize = 0xffff_ffff_ffff_f000;

// 1GB
pub const HARDWARD_BEGIN: usize = 0xffff_ffff_c000_0000;
pub const HARDWARD_END: usize = 0xffff_ffff_ffff_f000;

// 8MB
/// only used in init pagetable, then need to replace to range MEMORY
pub const INIT_MEMORY_SIZE: usize = 0x400_0000; // 8MB = 2^23
pub const INIT_MEMORY_END: usize = KERNEL_TEXT_BEGIN + INIT_MEMORY_SIZE;

// 1GB
pub const KERNEL_TEXT_BEGIN: usize = 0xffff_ffff_8000_0000;
pub const KERNEL_TEXT_END: usize = 0xffff_ffff_c000_0000;

/// 32GB
///
/// MEMORY_BEGIN mapping to 0x0
///
/// KERNEL_TEXT_BEGIN in 0xffff_fff0_8000_0000 need to mapping in entry.asm
pub const DIRECT_MAP_BEGIN: usize = 0xffff_fff0_0000_0000;
pub const DIRECT_MAP_END: usize = 0xffff_fff8_0000_0000;
pub const DIRECT_MAP_SIZE: usize = DIRECT_MAP_END - DIRECT_MAP_BEGIN;
/// change a kernel text pointer to direct memory pointer by minus this.
///
/// ptr(kernel text) = ptr(direct memory) + this
pub const KERNEL_OFFSET_FROM_DIRECT_MAP: usize =
    (KERNEL_TEXT_BEGIN - PHYSICAL_KERNEL_TEXT_BEGIN) - DIRECT_MAP_BEGIN;
/// eliminate init memory, previous space had been used.
pub const MEMORY_INIT_KERNEL_END: usize = INIT_MEMORY_END - KERNEL_OFFSET_FROM_DIRECT_MAP;

// 64GB
pub const IOMAP_BEGIN: usize = 0xffff_ffd0_0000_0000;
pub const IOMAP_END: usize = 0xffff_ffe0_0000_0000;

// total range: 256GB
pub const KERNEL_BASE: usize = 0xffff_ffc0_0000_0000;

pub const PHYSICAL_KERNEL_TEXT_BEGIN: usize = 0x8000_0000;
pub const DIRECT_MAP_OFFSET: usize = DIRECT_MAP_BEGIN;

/// ============================== USER ==============================
pub const USER_BASE: usize = 0x10000;
/// 32GB
pub const USER_DATA_BEGIN: usize = 0x10000;
pub const USER_DATA_END: usize = 0x8_0000_0000;
/// 32GB
pub const USER_HEAP_BEGIN: usize = 0x8_0000_0000;
pub const USER_HEAP_END: usize = 0x10_0000_0000;
/// 64GB
pub const USER_STACK_BEGIN: usize = 0x10_0000_0000;
pub const USER_STACK_END: usize = 0x20_0000_0000;
pub const USER_MAX_THREADS: usize = (USER_STACK_END - USER_STACK_BEGIN) / USER_STACK_SIZE;
/// full range of user
pub const USER_MMAP_BEGIN: usize = USER_DATA_BEGIN;
pub const USER_MMAP_SEARCH: usize = 0x20_0000_0000;
pub const USER_MMAP_END: usize = USER_END;

pub const USER_MMAP_RANGE: URange =
    UserAddr::<u8>::from(USER_MMAP_BEGIN).floor()..UserAddr::<u8>::from(USER_MMAP_END).ceil();

pub const USER_MMAP_SEARCH_RANGE: URange =
    UserAddr::<u8>::from(USER_MMAP_SEARCH).floor()..UserAddr::<u8>::from(USER_MMAP_END).ceil();

pub const USER_END: usize = 0x40_0000_0000;
