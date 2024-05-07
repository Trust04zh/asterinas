// SPDX-License-Identifier: MPL-2.0

//! Virtio over MMIO

pub mod bus;
pub mod device;

use alloc::vec::Vec;
use core::ops::Range;

#[cfg(feature = "intel_tdx")]
use ::tdx_guest::tdx_is_enabled;
use log::debug;

use self::bus::MmioBus;
#[cfg(feature = "intel_tdx")]
use crate::arch::tdx_guest;
use crate::{
    bus::mmio::device::MmioCommonDevice, sync::SpinLock, trap::IrqLine,
    vm::paddr_to_vaddr,
};

const VIRTIO_MMIO_MAGIC: u32 = 0x74726976;

pub static MMIO_BUS: SpinLock<MmioBus> = SpinLock::new(MmioBus::new());
static IRQS: SpinLock<Vec<IrqLine>> = SpinLock::new(Vec::new());

pub fn init() {
    #[cfg(feature = "intel_tdx")]
    // Safety:
    // This is safe because we are ensuring that the address range 0xFEB0_0000 to 0xFEB0_4000 is valid before this operation.
    // The address range is page-aligned and falls within the MMIO range, which is a requirement for the `unprotect_gpa_range` function.
    // We are also ensuring that we are only unprotecting four pages.
    // Therefore, we are not causing any undefined behavior or violating any of the requirements of the `unprotect_gpa_range` function.
    if tdx_is_enabled() {
        unsafe {
            tdx_guest::unprotect_gpa_range(0xFEB0_0000, 4).unwrap();
        }
    }
    // FIXME: The address 0xFEB0_0000 is obtained from an instance of microvm, and it may not work in other architecture.
    // iter_range(0xFEB0_0000..0xFEB0_4000);

    #[cfg(target_arch = "riscv64")]
    mmio_probe();
}

#[cfg(target_arch = "riscv64")]
fn mmio_probe() {
    use crate::arch::boot::DEVICE_TREE;

    let mut lock = MMIO_BUS.lock();
    for node in DEVICE_TREE.get().unwrap().find_all_nodes("/soc/virtio_mmio") {
        let reg = node.reg().unwrap().next().unwrap();
        let interrupt = node.interrupts().unwrap().next().unwrap();
        let handle = IrqLine::alloc_specific(interrupt as u8).unwrap();
        log::debug!("Initialize Virtio MMIO at {:#x?}, interrupt: {}", reg.starting_address, interrupt);

        let device = MmioCommonDevice::new(reg.starting_address as usize, handle);
        lock.register_mmio_device(device);
    }
}

#[cfg(target_arch = "x86_64")]
fn iter_range(range: Range<usize>) {
    use crate::arch::kernel::IO_APIC;
    debug!("[Virtio]: Iter MMIO range:{:x?}", range);
    let mut current = range.end;
    let mut lock = MMIO_BUS.lock();
    let io_apics = IO_APIC.get().unwrap();
    let is_ioapic2 = io_apics.len() == 2;
    let mut io_apic = if is_ioapic2 {
        io_apics.get(1).unwrap().lock()
    } else {
        io_apics.first().unwrap().lock()
    };
    let mut device_count = 0;
    while current > range.start {
        current -= 0x100;
        // Safety: It only read the value and judge if the magic value fit 0x74726976
        let value = unsafe { *(paddr_to_vaddr(current) as *const u32) };
        if value == VIRTIO_MMIO_MAGIC {
            // Safety: It only read the device id
            let device_id = unsafe { *(paddr_to_vaddr(current + 8) as *const u32) };
            device_count += 1;
            if device_id == 0 {
                continue;
            }
            let handle = IrqLine::alloc().unwrap();
            // If has two IOApic, then start: 24 (0 in IOApic2), end 47 (23 in IOApic2)
            // If one IOApic, then start: 16, end 23
            io_apic.enable(24 - device_count, handle.clone()).unwrap();
            let device = MmioCommonDevice::new(current, handle);
            lock.register_mmio_device(device);
        }
    }
}
