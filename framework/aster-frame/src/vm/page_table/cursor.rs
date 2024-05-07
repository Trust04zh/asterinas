// SPDX-License-Identifier: MPL-2.0

use alloc::{boxed::Box, sync::Arc};
use core::{marker::PhantomData, ops::Range};

use super::{
    MapInfo, MapOp, MapProperty, PageTable, PageTableConstsTrait, PageTableEntryTrait,
    PageTableError, PageTableFrame, PageTableMode, PtfRef,
};
use crate::{
    sync::SpinLock,
    vm::{paddr_to_vaddr, Paddr, Vaddr},
};

/// The cursor for traversal over the page table.
///
/// Doing mapping is somewhat like a depth-first search on a tree, except
/// that we modify the tree while traversing it. We use a stack to simulate
/// the recursion.
pub(super) struct PageTableCursor<
    'a,
    M: PageTableMode,
    E: PageTableEntryTrait,
    C: PageTableConstsTrait,
> where
    [(); C::NR_ENTRIES_PER_FRAME]:,
    [(); C::NR_LEVELS]:,
{
    stack: [Option<PtfRef<E, C>>; C::NR_LEVELS],
    level: usize,
    va: Vaddr,
    _phantom_ref: PhantomData<&'a PageTable<M, E, C>>,
}

impl<M: PageTableMode, E: PageTableEntryTrait, C: PageTableConstsTrait> PageTableCursor<'_, M, E, C>
where
    [(); C::NR_ENTRIES_PER_FRAME]:,
    [(); C::NR_LEVELS]:,
{
    pub(super) fn new(pt: &PageTable<M, E, C>, va: Vaddr) -> Self {
        let mut stack = core::array::from_fn(|_| None);
        stack[0] = Some(pt.root_frame.clone());
        Self {
            stack,
            level: C::NR_LEVELS,
            va,
            _phantom_ref: PhantomData,
        }
    }

    /// Map or unmap the range starting from the current address.
    ///
    /// The argument `create` allows you to map the continuous range to a physical
    /// range with the given map property.
    ///
    /// The function will map as more huge pages as possible, and it will split
    /// the huge pages into smaller pages if necessary. If the input range is large,
    /// the resulting mappings may look like this (if very huge pages supported):
    ///
    /// ```text
    /// start                                                             end
    ///   |----|----------------|--------------------------------|----|----|
    ///    base      huge                     very huge           base base
    ///    4KiB      2MiB                       1GiB              4KiB  4KiB
    /// ```
    ///
    /// In practice it is suggested to use simple wrappers for this API that maps
    /// frames for safety and conciseness.
    ///
    /// # Safety
    ///
    /// This function manipulates the page table directly, and it is unsafe because
    /// it may cause undefined behavior if the caller does not ensure that the
    /// mapped address is valid and the page table is not corrupted if it is used
    /// by the kernel.
    pub(super) unsafe fn map(&mut self, len: usize, create: Option<(Paddr, MapProperty)>) {
        let end = self.va + len;
        let mut create = create;
        while self.va != end {
            let top_spin = self.stack[C::NR_LEVELS - self.level].clone().unwrap();
            let mut top_ptf = top_spin.lock();
            // Go down if the page size is too big or alignment is not satisfied.
            let is_pa_not_aligned = create
                .map(|(pa, _)| pa % C::page_size(self.level) != 0)
                .unwrap_or(false);
            if self.level > C::HIGHEST_TRANSLATION_LEVEL
                || self.va % C::page_size(self.level) != 0
                || self.va + C::page_size(self.level) > end
                || is_pa_not_aligned
            {
                let ld_prop = create
                    .map(|(pa, prop)| prop)
                    .unwrap_or(MapProperty::new_invalid());
                self.level_down(&mut top_ptf, Some(ld_prop));
                continue;
            }
            self.map_page(&mut top_ptf, create);
            create = create.map(|(pa, prop)| (pa + C::page_size(self.level), prop));
            self.next_slot();
        }
    }

    /// Apply the given operation to all the mappings within the range.
    pub(super) unsafe fn protect(
        &mut self,
        len: usize,
        op: impl MapOp,
    ) -> Result<(), PageTableError> {
        let end = self.va + len;
        while self.va != end {
            let top_spin = self.stack[C::NR_LEVELS - self.level].clone().unwrap();
            let mut top_ptf = top_spin.lock();
            let cur_pte = self.cur_pte(&top_ptf);
            if !cur_pte.is_valid() {
                return Err(PageTableError::ProtectInvalid);
            }
            // Go down if it's not a last node.
            if !(cur_pte.is_huge() || self.level == 1)
                || (self.va % C::page_size(self.level)) != 0
                || self.va + C::page_size(self.level) > end
            {
                self.level_down(&mut top_ptf, Some(op(cur_pte.info())));
                continue;
            }
            // Apply the operation.
            *self.cur_pte_mut(&mut top_ptf) =
                E::new(cur_pte.paddr(), op(cur_pte.info()), cur_pte.is_huge(), true);
            self.next_slot();
        }
        Ok(())
    }

    fn cur_pte<'c, 'f, 'r: 'c + 'f>(&'c self, ptf: &'f PageTableFrame<E, C>) -> &'r E {
        let frame_addr = paddr_to_vaddr(ptf.inner.start_paddr());
        let offset = C::in_frame_index(self.va, self.level);
        // Safety: no outlive, no overflows.
        unsafe { &*(frame_addr as *const E).add(offset) }
    }

    fn cur_pte_mut<'c, 'f, 'r: 'c + 'f>(&'c self, ptf: &'f mut PageTableFrame<E, C>) -> &'r mut E {
        let frame_addr = paddr_to_vaddr(ptf.inner.start_paddr());
        let offset = C::in_frame_index(self.va, self.level);
        // Safety: no outlive, no overflows.
        unsafe { &mut *(frame_addr as *mut E).add(offset) }
    }

    /// Traverse forward in the current level to the next PTE.
    /// If reached the end of a page table frame, it leads itself to the parent frame.
    fn next_slot(&mut self) {
        self.va += C::page_size(self.level);
        while C::in_frame_index(self.va, self.level) == 0 {
            self.level_up();
        }
    }

    /// A level up operation during traversal. It usually happens when completing
    /// the traversal a child PT frame and go back to the parent PT frame.
    fn level_up(&mut self) {
        self.stack[C::NR_LEVELS - self.level] = None;
        self.level += 1;
    }

    /// A level down operation during traversal. It may split a huge page into
    /// smaller pages if we have an end address within the next mapped huge page.
    /// It may also create a new child frame if the current frame does not have one.
    /// If that may happen the map property of intermediate level `prop` should be
    /// passed in correctly. Whether the map property matters in an intermediate
    /// level is architecture-dependent.
    unsafe fn level_down(&mut self, top_ptf: &mut PageTableFrame<E, C>, prop: Option<MapProperty>) {
        let huge_pa_prop = {
            let pte = self.cur_pte(top_ptf);
            if pte.is_valid() && pte.is_huge() {
                Some((pte.paddr(), pte.info().prop))
            } else {
                None
            }
        };
        if top_ptf.child.is_none() {
            top_ptf.child = Some(Box::new(core::array::from_fn(|_| None)));
        };
        let nxt_lvl_frame = if let Some(nxt_lvl_frame) =
            top_ptf.child.as_ref().unwrap()[C::in_frame_index(self.va, self.level)].clone()
        {
            nxt_lvl_frame
        } else {
            let new_frame = PageTableFrame::<E, C>::new();
            // If it already maps a huge page, we should split it.
            if let Some((pa, prop)) = huge_pa_prop {
                for i in 0..C::NR_ENTRIES_PER_FRAME {
                    let nxt_level = self.level - 1;
                    let nxt_pte = {
                        let frame_addr = paddr_to_vaddr(new_frame.inner.start_paddr());
                        &mut *(frame_addr as *mut E).add(i)
                    };
                    *nxt_pte = E::new(pa + i * C::page_size(nxt_level), prop, nxt_level > 1, true);
                }
                *self.cur_pte_mut(top_ptf) =
                    E::new(new_frame.inner.start_paddr(), prop, false, false);
            } else {
                *self.cur_pte_mut(top_ptf) =
                    E::new(new_frame.inner.start_paddr(), prop.unwrap(), false, false);
            }
            let new_frame_ref = Arc::new(SpinLock::new(new_frame));
            top_ptf.child.as_mut().unwrap()[C::in_frame_index(self.va, self.level)] =
                Some(new_frame_ref.clone());
            top_ptf.child_count += 1;
            new_frame_ref
        };
        self.stack[C::NR_LEVELS - self.level + 1] = Some(nxt_lvl_frame);
        self.level -= 1;
    }

    /// Map or unmap the page pointed to by the cursor (which could be large).
    /// If the physical address and the map property are not provided, it unmaps
    /// the current page.
    unsafe fn map_page(
        &mut self,
        top_ptf: &mut PageTableFrame<E, C>,
        create: Option<(Paddr, MapProperty)>,
    ) {
        if let Some((pa, prop)) = create {
            *self.cur_pte_mut(top_ptf) = E::new(pa, prop, self.level > 1, true);
        } else {
            *self.cur_pte_mut(top_ptf) = E::new_invalid();
        }
        // If it dismantle a child we ensure it to be released.
        let dismantled = if let Some(child) = &mut top_ptf.child {
            let idx = C::in_frame_index(self.va, self.level);
            if child[idx].is_some() {
                child[idx] = None;
                true
            } else {
                false
            }
        } else {
            false
        };
        if dismantled {
            top_ptf.child_count -= 1;
            if top_ptf.child_count == 0 {
                top_ptf.child = None;
            }
        }
    }
}

/// The iterator for querying over the page table without modifying it.
pub struct PageTableIter<'a, M: PageTableMode, E: PageTableEntryTrait, C: PageTableConstsTrait>
where
    [(); C::NR_ENTRIES_PER_FRAME]:,
    [(); C::NR_LEVELS]:,
{
    cursor: PageTableCursor<'a, M, E, C>,
    end_va: Vaddr,
}

impl<'a, M: PageTableMode, E: PageTableEntryTrait, C: PageTableConstsTrait>
    PageTableIter<'a, M, E, C>
where
    [(); C::NR_ENTRIES_PER_FRAME]:,
    [(); C::NR_LEVELS]:,
{
    pub(super) fn new(pt: &'a PageTable<M, E, C>, va: &Range<Vaddr>) -> Self {
        Self {
            cursor: PageTableCursor::new(pt, va.start),
            end_va: va.end,
        }
    }
}

pub struct PageTableQueryResult {
    pub va: Range<Vaddr>,
    pub info: MapInfo,
}

impl<'a, M: PageTableMode, E: PageTableEntryTrait, C: PageTableConstsTrait> Iterator
    for PageTableIter<'a, M, E, C>
where
    [(); C::NR_ENTRIES_PER_FRAME]:,
    [(); C::NR_LEVELS]:,
{
    type Item = PageTableQueryResult;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor.va >= self.end_va {
            return None;
        }
        loop {
            let level = self.cursor.level;
            let va = self.cursor.va;
            let top_spin = self.cursor.stack[C::NR_LEVELS - level].clone().unwrap();
            let mut top_ptf = top_spin.lock();
            let cur_pte = self.cursor.cur_pte(&top_ptf);
            // Yeild if it's not a valid node.
            if !cur_pte.is_valid() {
                return None;
            }
            // Go down if it's not a last node.
            if !(cur_pte.is_huge() || level == 1) {
                // Safety: alignment checked and there should be a child frame here.
                unsafe {
                    self.cursor.level_down(&mut top_ptf, None);
                }
                continue;
            }
            // Yield the current mapping.
            let mapped_range = self.cursor.va..self.cursor.va + C::page_size(self.cursor.level);
            let map_info = cur_pte.info();
            self.cursor.next_slot();
            return Some(PageTableQueryResult {
                va: mapped_range,
                info: map_info,
            });
        }
    }
}
