// SPDX-License-Identifier: MPL-2.0

use aster_frame::{cpu::*, vm::VmIo};

use crate::{
    prelude::*, process::signal::signals::fault::FaultSignal,
    vm::page_fault_handler::PageFaultHandler,
};

/// We can't handle most exceptions, just send self a fault signal before return to user space.
pub fn handle_exception(context: &UserContext) {
    let trap_info = context.trap_information();
    let exception = trap_info.code;
    log_trap_info(exception, trap_info);
    let current = current!();
    let root_vmar = current.root_vmar();

    match exception {
        CpuException::InstructionPageFault | CpuException::LoadPageFault | CpuException::StorePageFault => handle_page_fault(trap_info),
        _ => {
            // We current do nothing about other exceptions
            generate_fault_signal(trap_info);
        }
    }
}

fn handle_page_fault(trap_info: &CpuExceptionInfo) {
    const PAGE_NOT_PRESENT_ERROR_MASK: usize = 0x1 << 0;
    const WRITE_ACCESS_MASK: usize = 0x1 << 1;
    let page_fault_addr = trap_info.page_fault_addr as Vaddr;
    trace!(
        "page fault error code: 0x{:x}, Page fault address: 0x{:x}",
        trap_info.error_code,
        page_fault_addr
    );
    let not_present = trap_info.error_code & PAGE_NOT_PRESENT_ERROR_MASK == 0;
    let write = trap_info.code == CpuException::StorePageFault;
    if not_present || write {
        // If page is not present or due to write access, we should ask the vmar try to commit this page
        let current = current!();
        let root_vmar = current.root_vmar();
        if let Err(e) = root_vmar.handle_page_fault(page_fault_addr, not_present, write) {
            error!(
                "page fault handler failed: addr: 0x{:x}, err: {:?}",
                page_fault_addr, e
            );
            generate_fault_signal(trap_info);
        } else {
            // ensure page fault is successfully handled
            // FIXME: this check can be removed
            let vm_space = root_vmar.vm_space();
            let _: u8 = vm_space.read_val(page_fault_addr).unwrap();
        }
    } else {
        // Otherwise, the page fault cannot be handled
        generate_fault_signal(trap_info);
    }
}

/// generate a fault signal for current process.
fn generate_fault_signal(trap_info: &CpuExceptionInfo) {
    let current = current!();
    let signal = FaultSignal::new(trap_info);
    current.enqueue_signal(signal);
}

#[cfg(target_arch = "x86_64")]
fn log_trap_info(exception: &CpuException, trap_info: &CpuExceptionInfo) {
    macro_rules! log_trap_common {
        ($exception_name: ident, $trap_info: ident) => {
            trace!(
                "[Trap][{}][err = {}]",
                stringify!($exception_name),
                $trap_info.error_code
            )
        };
    }

    match *exception {
        DIVIDE_BY_ZERO => log_trap_common!(DIVIDE_BY_ZERO, trap_info),
        DEBUG => log_trap_common!(DEBUG, trap_info),
        NON_MASKABLE_INTERRUPT => log_trap_common!(NON_MASKABLE_INTERRUPT, trap_info),
        BREAKPOINT => log_trap_common!(BREAKPOINT, trap_info),
        OVERFLOW => log_trap_common!(OVERFLOW, trap_info),
        BOUND_RANGE_EXCEEDED => log_trap_common!(BOUND_RANGE_EXCEEDED, trap_info),
        INVALID_OPCODE => log_trap_common!(INVALID_OPCODE, trap_info),
        DEVICE_NOT_AVAILABLE => log_trap_common!(DEVICE_NOT_AVAILABLE, trap_info),
        DOUBLE_FAULT => log_trap_common!(DOUBLE_FAULT, trap_info),
        COPROCESSOR_SEGMENT_OVERRUN => log_trap_common!(COPROCESSOR_SEGMENT_OVERRUN, trap_info),
        INVAILD_TSS => log_trap_common!(INVAILD_TSS, trap_info),
        SEGMENT_NOT_PRESENT => log_trap_common!(SEGMENT_NOT_PRESENT, trap_info),
        STACK_SEGMENT_FAULT => log_trap_common!(STACK_SEGMENT_FAULT, trap_info),
        GENERAL_PROTECTION_FAULT => log_trap_common!(GENERAL_PROTECTION_FAULT, trap_info),
        PAGE_FAULT => {
            trace!(
                "[Trap][{}][page fault addr = 0x{:x}, err = {}]",
                stringify!(PAGE_FAULT),
                trap_info.page_fault_addr,
                trap_info.error_code
            );
        }
        // 15 reserved
        X87_FLOATING_POINT_EXCEPTION => log_trap_common!(X87_FLOATING_POINT_EXCEPTION, trap_info),
        ALIGNMENT_CHECK => log_trap_common!(ALIGNMENT_CHECK, trap_info),
        MACHINE_CHECK => log_trap_common!(MACHINE_CHECK, trap_info),
        SIMD_FLOATING_POINT_EXCEPTION => log_trap_common!(SIMD_FLOATING_POINT_EXCEPTION, trap_info),
        VIRTUALIZATION_EXCEPTION => log_trap_common!(VIRTUALIZATION_EXCEPTION, trap_info),
        CONTROL_PROTECTION_EXCEPTION => log_trap_common!(CONTROL_PROTECTION_EXCEPTION, trap_info),
        HYPERVISOR_INJECTION_EXCEPTION => {
            log_trap_common!(HYPERVISOR_INJECTION_EXCEPTION, trap_info)
        }
        VMM_COMMUNICATION_EXCEPTION => log_trap_common!(VMM_COMMUNICATION_EXCEPTION, trap_info),
        SECURITY_EXCEPTION => log_trap_common!(SECURITY_EXCEPTION, trap_info),
        _ => {
            info!(
                "[Trap][Unknown trap type][err = {}]",
                trap_info.error_code
            );
            // info!(
            //     "[Trap][Unknown trap type][id = {}, err = {}]",
            //     trap_info.id, trap_info.error_code
            // );
        }
    }
}

#[cfg(target_arch = "riscv64")]
fn log_trap_info(exception: CpuException, trap_info: &CpuExceptionInfo) {
    macro_rules! log_trap_common {
        ($exception_name: ident, $trap_info: ident) => {
            trace!(
                "[Trap][{}][err = {:?}]",
                stringify!($exception_name),
                $trap_info.code
            )
        };
    }

    match exception {
        CpuException::InstructionMisaligned => log_trap_common!(InstructionMisaligned, trap_info),
        CpuException::InstructionFault => log_trap_common!(InstructionFault, trap_info),
        CpuException::IllegalInstruction => log_trap_common!(IllegalInstruction, trap_info),
        CpuException::Breakpoint => log_trap_common!(Breakpoint, trap_info),
        CpuException::LoadMisaligned => log_trap_common!(LoadMisaligned, trap_info),
        CpuException::LoadFault => log_trap_common!(LoadFault, trap_info),
        CpuException::StoreMisaligned => log_trap_common!(StoreMisaligned, trap_info),
        CpuException::StoreFault => log_trap_common!(StoreFault, trap_info),
        CpuException::UserEnvCall => log_trap_common!(UserEnvCall, trap_info),
        CpuException::SupervisorEnvCall => log_trap_common!(SupervisorEnvCall, trap_info),
        CpuException::InstructionPageFault => {
            trace!(
                "[Trap][{}][page fault addr = 0x{:x}, err = {:?}]",
                stringify!(InstructionPageFault),
                trap_info.page_fault_addr,
                trap_info.code
            );
        }
        CpuException::LoadPageFault => {
            trace!(
                "[Trap][{}][page fault addr = 0x{:x}, err = {:?}]",
                stringify!(LoadPageFault),
                trap_info.page_fault_addr,
                trap_info.code
            );
        }
        CpuException::StorePageFault => {
            trace!(
                "[Trap][{}][page fault addr = 0x{:x}, err = {:?}]",
                stringify!(StorePageFault),
                trap_info.page_fault_addr,
                trap_info.code
            );
        }
        CpuException::Unknown => log_trap_common!(Unknown, trap_info),
    }
}
