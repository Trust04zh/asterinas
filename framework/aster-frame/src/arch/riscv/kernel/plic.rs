// SPDX-License-Identifier: MPL-2.0

use core::{cell::UnsafeCell, num::NonZeroU32};
use spin::Once;

use bit_field::BitField;

use crate::{arch::boot::DEVICE_TREE, cpu::{this_cpu, this_plic_context}, vm::paddr_to_vaddr};

pub const MAX_INTERRUPT: usize = 1024; // 1-1023, 0 is reserved
pub const MAX_CONTEXT: usize = 15872;

pub const MAX_INTERRUPT_WORDS: usize = MAX_INTERRUPT / 32;

pub struct Writable;

#[repr(C, align(4096))]
pub struct Plic {
    pub priorities: [UnsafeCell<u32>; MAX_INTERRUPT],
    pub pendings: Bits<MAX_INTERRUPT_WORDS, ()>,
    _reserved: [u8; 0xf80],
    pub enables: [Bits<MAX_INTERRUPT_WORDS, Writable>; MAX_CONTEXT],
    _reserved2: [u8; 0xe000],
    pub contexts: [ContextLocal; MAX_CONTEXT],
}

unsafe impl Sync for Plic {}

#[repr(C, align(4096))]
pub struct ContextLocal {
    pub threshold: UnsafeCell<u32>,
    pub claim: UnsafeCell<u32>,
    _reserved: [u8; 0xff8],
}

pub(crate) static PLIC: Once<&'static Plic> = Once::new();

pub fn init() {
    let node = DEVICE_TREE.get().unwrap().find_node("/soc/plic").unwrap();
    let reg = node.reg().unwrap().next().unwrap();
    log::debug!("Initialize PLIC at {:#x?}", reg.starting_address);
    let addr = paddr_to_vaddr(reg.starting_address as usize);
    let plic = unsafe { &*(addr as *const Plic) };

    let phandle = node.property("phandle").unwrap().as_usize().unwrap();
    let context_id = this_plic_context();
    // Find all devices that managed by this PLIC and enable them on this context.
    // TODO: enable and handle interrupts in all harts.
    for node in DEVICE_TREE.get().unwrap().all_nodes() {
        let interrupt_parent = node.property("interrupt-parent").and_then(|p| p.as_usize());
        if interrupt_parent == Some(phandle) && let Some(interrupts) = node.interrupts() {
            for interrupt in interrupts {
                plic.enable(context_id, interrupt as u32);
                plic.set_interrupt_priority(interrupt as u32, 1);
            }
        }
    }
    plic.set_priority_threshold(context_id, 0);

    // Enable external interrupt
    unsafe { riscv::register::sie::set_sext(); }

    PLIC.call_once(|| plic);
}

impl Plic {
    pub fn enable(&self, context: usize, interrupt: u32) {
        log::trace!("PLIC enable interrupt {interrupt} for context {context}");
        self.enables[context].set(interrupt as usize);
    }

    pub fn disable(&self, context: usize, interrupt: u32) {
        self.enables[context].clear(interrupt as usize);
    }

    pub fn claim(&self, context: usize) -> Option<u32> {
        let claim_addr = self.contexts[context].claim.get();
        let claim = unsafe { claim_addr.read_volatile() };
        match claim {
            0 => None,
            _ => Some(claim),
        }
    }

    pub fn complete(&self, context: usize, interrupt: u32) {
        assert_ne!(interrupt, 0, "interrupt 0 is reserved");
        let claim_addr = self.contexts[context].claim.get();
        unsafe {
            claim_addr.write_volatile(interrupt);
        }
    }

    pub fn set_interrupt_priority(&self, interrupt: u32, priority: u32) {
        log::trace!("PLIC set interrupt {interrupt} priority {priority}");
        let priority_addr = self.priorities[interrupt as usize].get();
        unsafe {
            priority_addr.write_volatile(priority);
        }
    }

    pub fn set_priority_threshold(&self, context: usize, threshold: u32) {
        log::trace!("PLIC set priority threshold {threshold} for context {context}");
        let threshold_addr = self.contexts[context].threshold.get();
        unsafe {
            threshold_addr.write_volatile(threshold);
        }
    }
}

#[repr(transparent)]
pub struct Bits<const SIZE: usize, RW>{
    pub data: [UnsafeCell<u32>; SIZE],
    _rw: core::marker::PhantomData<RW>,
}

impl<const SIZE: usize, RW> Bits<SIZE, RW> {
    pub fn get(&self, index: usize) -> bool {
        let word = index / 32;
        let bit = index % 32;
        let ptr = self.data[word].get();
        unsafe { ptr.read_volatile().get_bit(bit) }
    }
}

impl<const SIZE: usize> Bits<SIZE, Writable> {
    pub fn write(&self, index: usize, val: bool) {
        let word = index / 32;
        let bit = index % 32;
        let ptr = self.data[word].get();
        unsafe {
            ptr.write_volatile(*ptr.read_volatile().set_bit(bit, val))
        }
    }

    pub fn clear(&self, index: usize) {
        self.write(index, false)
    }

    pub fn set(&self, index: usize) {
        self.write(index, true)
    }
}
