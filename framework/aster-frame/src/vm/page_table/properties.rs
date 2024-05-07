// SPDX-License-Identifier: MPL-2.0

use core::fmt::Debug;

use pod::Pod;

use crate::vm::{Paddr, Vaddr, VmPerm};

/// A minimal set of constants that determines the flags of the page table.
/// This provides an abstraction over most paging modes in common architectures.
pub trait PageTableConstsTrait: Debug {
    /// The smallest page size.
    const BASE_PAGE_SIZE: usize;

    /// The number of levels in the page table.
    /// The level 1 is the leaf level, and the level `NR_LEVELS` is the root level.
    const NR_LEVELS: usize;

    /// The highest level that a PTE can be directly used to translate a VA.
    /// This affects the the largest page size supported by the page table.
    const HIGHEST_TRANSLATION_LEVEL: usize;

    /// The size of a PTE.
    const ENTRY_SIZE: usize;

    // Here are some const values that are determined by the page table constants.

    /// The number of PTEs per page table frame.
    const NR_ENTRIES_PER_FRAME: usize = Self::BASE_PAGE_SIZE / Self::ENTRY_SIZE;

    /// The number of bits used to index a PTE in a page table frame.
    const IN_FRAME_INDEX_BITS: usize = Self::NR_ENTRIES_PER_FRAME.ilog2() as usize;

    /// The index of a VA's PTE in a page table frame at the given level.
    fn in_frame_index(va: Vaddr, level: usize) -> usize {
        va >> (Self::BASE_PAGE_SIZE.ilog2() as usize + Self::IN_FRAME_INDEX_BITS * (level - 1))
            & (Self::NR_ENTRIES_PER_FRAME - 1)
    }

    /// The page size at a given level.
    fn page_size(level: usize) -> usize {
        Self::BASE_PAGE_SIZE << (Self::IN_FRAME_INDEX_BITS * (level - 1))
    }
}

bitflags::bitflags! {
    /// The status of a memory mapping recorded by the hardware.
    pub struct MapStatus: u8 {
        const ACCESSED = 0b0000_0001;
        const DIRTY    = 0b0000_0010;
    }
}

/// The cache policy of a memory mapping.
/// FIXME: This may not be supported by all architectures and could be
/// ignored by us without warnings at the moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MapCachePolicy {
    Uncachable,
    WriteCombining,
    WriteThrough,
    WriteBack,
    WriteProtected,
}

#[derive(Clone, Copy, Debug)]
pub struct MapProperty {
    pub perm: VmPerm,
    /// Global.
    /// A global page is not evicted from the TLB when TLB is flushed.
    pub global: bool,
    /// The properties of a memory mapping that is used and defined as flags in PTE
    /// in specific architectures on an ad hoc basis. The logics provided by the
    /// page table module will not be affected by this field.
    pub extension: u64,
    pub cache: MapCachePolicy,
}

/// Any functions that could be used to modify the map property of a memory mapping.
///
/// To protect a virtual address range, you can either directly use a `MapProperty` object,
///
/// ```rust
/// let page_table = KERNEL_PAGE_TABLE.get().unwrap().lock();
/// let prop = MapProperty {
///     perm: VmPerm::R,
///     global: true,
///     extension: 0,
///     cache: MapCachePolicy::WriteBack,
/// };
/// page_table.protect(0..PAGE_SIZE, prop);
/// ```
///
/// use a map operation
///
/// ```rust
/// let page_table = KERNEL_PAGE_TABLE.get().unwrap().lock();
/// page_table.map(0..PAGE_SIZE, cache_policy_op(MapCachePolicy::WriteBack));
/// page_table.map(0..PAGE_SIZE, perm_op(|perm| perm | VmPerm::R));
/// ```
///
/// or even customize a map operation using a closure
///
/// ```rust
/// let page_table = KERNEL_PAGE_TABLE.get().unwrap().lock();
/// page_table.map(0..PAGE_SIZE, |info| {
///     assert!(info.prop.perm.contains(VmPerm::R));
///     MapProperty {
///         perm: info.prop.perm | VmPerm::W,
///         global: info.prop.global,
///         extension: info.prop.extension,
///         cache: info.prop.cache,
///     }
/// });
/// ```
pub trait MapOp: Fn(MapInfo) -> MapProperty {}
impl<F> MapOp for F where F: Fn(MapInfo) -> MapProperty {}

// These implementations allow a property to be used as an overriding map operation.
// Other usages seems pointless.
impl FnOnce<(MapInfo,)> for MapProperty {
    type Output = MapProperty;
    extern "rust-call" fn call_once(self, _: (MapInfo,)) -> MapProperty {
        self
    }
}
impl FnMut<(MapInfo,)> for MapProperty {
    extern "rust-call" fn call_mut(&mut self, _: (MapInfo,)) -> MapProperty {
        *self
    }
}
impl Fn<(MapInfo,)> for MapProperty {
    extern "rust-call" fn call(&self, _: (MapInfo,)) -> MapProperty {
        *self
    }
}

/// A life saver for creating a map operation that sets the cache policy.
pub fn cache_policy_op(cache: MapCachePolicy) -> impl MapOp {
    move |info| MapProperty {
        perm: info.prop.perm,
        global: info.prop.global,
        extension: info.prop.extension,
        cache,
    }
}

/// A life saver for creating a map operation that adjusts the permission.
pub fn perm_op(op: impl Fn(VmPerm) -> VmPerm) -> impl MapOp {
    move |info| MapProperty {
        perm: op(info.prop.perm),
        global: info.prop.global,
        extension: info.prop.extension,
        cache: info.prop.cache,
    }
}

impl MapProperty {
    pub fn new_general(perm: VmPerm) -> Self {
        Self {
            perm,
            global: false,
            extension: 0,
            cache: MapCachePolicy::WriteBack,
        }
    }

    pub fn new_invalid() -> Self {
        Self {
            perm: VmPerm::empty(),
            global: false,
            extension: 0,
            cache: MapCachePolicy::Uncachable,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MapInfo {
    pub prop: MapProperty,
    pub status: MapStatus,
}

pub trait PageTableEntryTrait: Clone + Copy + Sized + Pod + Debug {
    /// Create a new invalid page table flags that causes page faults
    /// when the MMU meets them.
    fn new_invalid() -> Self;
    /// If the flags are valid.
    /// Note that the invalid PTE may be _valid_ in representation, but
    /// just causing page faults when the MMU meets them.
    fn is_valid(&self) -> bool;

    /// Create a new PTE with the given physical address and flags.
    /// The huge flag indicates that the PTE maps a huge page.
    /// The last flag indicates that the PTE is the last level page table.
    /// If the huge and last flags are both false, the PTE maps a page
    /// table frame.
    fn new(paddr: Paddr, prop: MapProperty, huge: bool, last: bool) -> Self;

    /// Get the physical address from the PTE.
    /// The physical address recorded in the PTE is either:
    /// - the physical address of the next level page table;
    /// - or the physical address of the page frame it maps to.
    fn paddr(&self) -> Paddr;

    fn info(&self) -> MapInfo;

    /// If the PTE maps a huge page or a page table frame.
    fn is_huge(&self) -> bool;
}
