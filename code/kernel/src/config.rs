#![allow(dead_code)]
use core::{ops::Range, time::Duration};

use crate::{memory::address::UserAddr, process::resource::RLimit, tools::range::URange};

/// how many time interrupt per second
pub const TIME_INTERRUPT_PER_SEC: usize = 10;

pub const USER_STACK_SIZE: usize = PAGE_SIZE * 64; // 256KB
pub const USER_STACK_RESERVE: usize = PAGE_SIZE; // 一开始就映射的用户栈大小
pub const KERNEL_STACK_SIZE: usize = PAGE_SIZE * 16; // 内核栈大小, 每个CPU一个
pub const USER_FNO_DEFAULT: RLimit = RLimit::new_equal(200); // 控制最大文件打开数量等的默认值
pub const FS_CACHE_MAX_SIZE: usize = 200; // vfs中缓存的inode数量

pub const IDIE_SPIN_TIME: Duration = Duration::from_millis(1); // 没有新任务且超过这个时间才会睡眠
/// ============================== KERNEL ==============================
///
/// 0x8_0000 = 512KB
/// 0x10_0000 = 1MB
pub const KERNEL_HEAP_SIZE: usize = 0x1_0000; // 64KB, 自动从帧分配器扩容

pub const PAGE_SIZE: usize = 0x1000; // 0x1000
pub const PAGE_SIZE_BITS: usize = 12; // 12
#[deprecated]
pub const TRAMPOLINE: usize = 0xffff_ffff_ffff_f000;

/// 1GB
pub const HARDWARD_BEGIN: usize = 0xffff_ffff_c000_0000;
pub const HARDWARD_END: usize = 0xffff_ffff_ffff_f000;

/// only used in init pagetable, then need to replace to range MEMORY, 1MB = 0x1_00000
///
/// 1MB = 0x100_000
///
/// 256MB = 0x10_000_000
///
pub const INIT_MEMORY_SIZE: usize = 0x1000_0000;
pub const INIT_MEMORY_END: usize = KERNEL_TEXT_BEGIN + INIT_MEMORY_SIZE;

/// 1GB
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

pub const USER_DYN_BEGIN: usize = 0x20_0000_0000;
pub const USER_DYN_END: usize = 0x30_0000_0000;
pub const USER_DYN_RANGE: URange = get_range(USER_DYN_BEGIN..USER_DYN_END);

/// full range of user
pub const USER_MMAP_BEGIN: usize = USER_DATA_BEGIN;
pub const USER_MMAP_SEARCH: usize = 0x30_0000_0000;
pub const USER_MMAP_END: usize = 0x38_0000_0000;

pub const USER_KRX_BEGIN: usize = USER_END - 0x10000 + 0x4000; // 放置提供给用户的一些代码
pub const USER_KRX_END: usize = USER_END - 0x10000 + 0x5000;

pub const USER_KRW_RANDOM_BEGIN: usize = USER_END - 0x10000 + 0x6000; // 写满了随机数的页
pub const USER_KRW_RANDOM_END: usize = USER_END - 0x10000 + 0x7000; //

pub const USER_MMAP_RANGE: URange = get_range(USER_MMAP_BEGIN..USER_MMAP_END);

pub const USER_MMAP_SEARCH_RANGE: URange = get_range(USER_MMAP_SEARCH..USER_MMAP_END);

pub const USER_KRX_RANGE: URange = get_range(USER_KRX_BEGIN..USER_KRX_END);
pub const USER_KRW_RANDOM_RANGE: URange = get_range(USER_KRW_RANDOM_BEGIN..USER_KRW_RANDOM_END);

pub const USER_END: usize = 0x40_0000_0000;

const fn get_range(range: Range<usize>) -> URange {
    UserAddr::<u8>::from(range.start).floor()..UserAddr::<u8>::from(range.end).ceil()
}
