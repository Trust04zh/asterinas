// SPDX-License-Identifier: MPL-2.0

use alloc::fmt;

use pod::Pod;
use x86_64::{instructions::tlb, structures::paging::PhysFrame, VirtAddr};

use crate::vm::{
    page_table::{
        MapCachePolicy, MapInfo, MapProperty, MapStatus, PageTableConstsTrait, PageTableEntryTrait,
    },
    Paddr, Vaddr, VmPerm,
};

pub(crate) const NR_ENTRIES_PER_PAGE: usize = 512;

#[derive(Debug)]
pub struct PageTableConsts {}

impl PageTableConstsTrait for PageTableConsts {
    const BASE_PAGE_SIZE: usize = 4096;
    const NR_LEVELS: usize = 4;
    const HIGHEST_TRANSLATION_LEVEL: usize = 2;
    const ENTRY_SIZE: usize = core::mem::size_of::<PageTableEntry>();
}

bitflags::bitflags! {
    #[derive(Pod)]
    #[repr(C)]
    /// Possible flags for a page table entry.
    pub struct PageTableFlags: usize {
        /// Specifies whether the mapped frame or page table is loaded in memory.
        const PRESENT =         1 << 0;
        /// Controls whether writes to the mapped frames are allowed.
        const WRITABLE =        1 << 1;
        /// Controls whether accesses from userspace (i.e. ring 3) are permitted.
        const USER =            1 << 2;
        /// If this bit is set, a “write-through” policy is used for the cache, else a “write-back”
        /// policy is used.
        const WRITE_THROUGH =   1 << 3;
        /// Disables caching for the pointed entry is cacheable.
        const NO_CACHE =        1 << 4;
        /// Whether this entry has been used for linear-address translation.
        const ACCESSED =        1 << 5;
        /// Whether the memory area represented by this entry is modified.
        const DIRTY =           1 << 6;
        /// Only in the non-starting and non-ending levels, indication of huge page.
        const HUGE =            1 << 7;
        /// Indicates that the mapping is present in all address spaces, so it isn't flushed from
        /// the TLB on an address space switch.
        const GLOBAL =          1 << 8;
        /// TDX shared bit.
        #[cfg(feature = "intel_tdx")]
        const SHARED =          1 << 51;
        /// Forbid execute codes on the page. The NXE bits in EFER msr must be set.
        const NO_EXECUTE =      1 << 63;
    }
}

pub fn tlb_flush(vaddr: Vaddr) {
    tlb::flush(VirtAddr::new(vaddr as u64));
}

#[derive(Clone, Copy, Pod)]
#[repr(C)]
pub struct PageTableEntry(usize);

/// Activate the given level 4 page table.
/// The cache policy of the root page table frame is controlled by `root_pt_cache`.
///
/// ## Safety
///
/// Changing the level 4 page table is unsafe, because it's possible to violate memory safety by
/// changing the page mapping.
pub unsafe fn activate_page_table(root_paddr: Paddr, root_pt_cache: MapCachePolicy) {
    x86_64::registers::control::Cr3::write(
        PhysFrame::from_start_address(x86_64::PhysAddr::new(root_paddr as u64)).unwrap(),
        match root_pt_cache {
            MapCachePolicy::WriteBack => x86_64::registers::control::Cr3Flags::empty(),
            MapCachePolicy::WriteThrough => {
                x86_64::registers::control::Cr3Flags::PAGE_LEVEL_WRITETHROUGH
            }
            MapCachePolicy::Uncachable => {
                x86_64::registers::control::Cr3Flags::PAGE_LEVEL_CACHE_DISABLE
            }
            _ => panic!("unsupported cache policy for the root page table"),
        },
    );
}

pub fn current_page_table_paddr() -> Paddr {
    x86_64::registers::control::Cr3::read()
        .0
        .start_address()
        .as_u64() as Paddr
}

impl PageTableEntry {
    /// 51:12
    #[cfg(not(feature = "intel_tdx"))]
    const PHYS_ADDR_MASK: usize = 0xF_FFFF_FFFF_F000;
    #[cfg(feature = "intel_tdx")]
    const PHYS_ADDR_MASK: usize = 0x7_FFFF_FFFF_F000;
}

impl PageTableEntryTrait for PageTableEntry {
    fn new_invalid() -> Self {
        Self(0)
    }

    fn is_valid(&self) -> bool {
        self.0 & PageTableFlags::PRESENT.bits() != 0
    }

    fn new(paddr: Paddr, prop: MapProperty, huge: bool, last: bool) -> Self {
        let mut flags = PageTableFlags::PRESENT;
        if !huge && !last {
            // In x86 if it's an intermediate PTE, it's better to have the same permissions
            // as the most permissive child (to reduce hardware page walk accesses). But we
            // don't have a mechanism to keep it generic across architectures, thus just
            // setting it to be the most permissive.
            flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER;
        } else {
            if prop.perm.contains(VmPerm::W) {
                flags |= PageTableFlags::WRITABLE;
            }
            if !prop.perm.contains(VmPerm::X) {
                flags |= PageTableFlags::NO_EXECUTE;
            }
            if prop.perm.contains(VmPerm::U) {
                flags |= PageTableFlags::USER;
            }
            if prop.global {
                flags |= PageTableFlags::GLOBAL;
            }
        }
        if prop.cache == MapCachePolicy::Uncachable {
            flags |= PageTableFlags::NO_CACHE;
        }
        if prop.cache == MapCachePolicy::WriteThrough {
            flags |= PageTableFlags::WRITE_THROUGH;
        }
        if huge {
            flags |= PageTableFlags::HUGE;
        }
        #[cfg(feature = "intel_tdx")]
        if prop.extension as usize & PageTableFlags::SHARED.bits() != 0 {
            flags |= PageTableFlags::SHARED;
        }
        Self(paddr & Self::PHYS_ADDR_MASK | flags.bits())
    }

    fn paddr(&self) -> Paddr {
        self.0 & Self::PHYS_ADDR_MASK
    }

    fn info(&self) -> MapInfo {
        let mut perm = VmPerm::empty();
        if self.0 & PageTableFlags::PRESENT.bits() != 0 {
            perm |= VmPerm::R;
        }
        if self.0 & PageTableFlags::WRITABLE.bits() != 0 {
            perm |= VmPerm::W;
        }
        if self.0 & PageTableFlags::NO_EXECUTE.bits() == 0 {
            perm |= VmPerm::X;
        }
        if self.0 & PageTableFlags::USER.bits() != 0 {
            perm |= VmPerm::U;
        }
        let global = self.0 & PageTableFlags::GLOBAL.bits() != 0;
        let cache = if self.0 & PageTableFlags::NO_CACHE.bits() != 0 {
            MapCachePolicy::Uncachable
        } else if self.0 & PageTableFlags::WRITE_THROUGH.bits() != 0 {
            MapCachePolicy::WriteThrough
        } else {
            MapCachePolicy::WriteBack
        };
        let mut status = MapStatus::empty();
        if self.0 & PageTableFlags::ACCESSED.bits() != 0 {
            status |= MapStatus::ACCESSED;
        }
        if self.0 & PageTableFlags::DIRTY.bits() != 0 {
            status |= MapStatus::DIRTY;
        }
        MapInfo {
            prop: MapProperty {
                perm,
                global,
                extension: (self.0 & !Self::PHYS_ADDR_MASK) as u64,
                cache,
            },
            status,
        }
    }

    fn is_huge(&self) -> bool {
        self.0 & PageTableFlags::HUGE.bits() != 0
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut f = f.debug_struct("PageTableEntry");
        f.field("raw", &format_args!("{:#x}", self.0))
            .field("paddr", &format_args!("{:#x}", self.paddr()))
            .field("valid", &self.is_valid())
            .field(
                "flags",
                &PageTableFlags::from_bits_truncate(self.0 & !Self::PHYS_ADDR_MASK),
            )
            .field("info", &self.info())
            .finish()
    }
}
