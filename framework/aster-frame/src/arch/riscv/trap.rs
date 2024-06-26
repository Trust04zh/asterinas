// SPDX-License-Identifier: MPL-2.0

use core::{num::NonZeroU32, sync::atomic::{AtomicBool, Ordering}};

use trapframe::TrapFrame;

use crate::{cpu::{this_cpu, this_plic_context}, cpu_local};

use super::{irq::IRQ_LIST, kernel::plic::PLIC};

/// Only from kernel
#[no_mangle]
extern "C" fn trap_handler(f: &mut TrapFrame) {
    use riscv::register::scause::{Interrupt::*, Trap};

    match riscv::register::scause::read().cause() {
        Trap::Interrupt(SupervisorExternal) => {
            call_irq_callback_functions(f);
        }
        Trap::Interrupt(_) => unimplemented!(),
        Trap::Exception(e) => {
            let stval = riscv::register::stval::read();
            panic!(
                "Cannot handle kernel cpu exception: {e:?}. stval: {stval:#x}, trapframe: {f:#x?}.",
            );
        }
    }
}

pub(crate) fn call_irq_callback_functions(trap_frame: &TrapFrame) {
    // For RISC-V, interrupts are not set re-entrant by default. Local interrupts will be disabled when
    // an interrupt handler is called (Unless interrupts are re-enabled in an interrupt handler).
    //
    // FIXME: For arch that supports re-entrant interrupts, we may need to record nested level here.
    IN_INTERRUPT_CONTEXT.store(true, Ordering::Release);

    let plic = PLIC.get().unwrap();
    let context_id = this_plic_context();

    // After the highest-priority pending interrupt is claimed by a target and the corresponding
    // IP bit is cleared, other lower-priority pending interrupts might then become visible to the
    // target. (PLIC Spec 1.0.0)
    // So we need to poll until there is no pending interrupt.
    while let Some(interrupt) = plic.claim(context_id) {
        let irq_line = IRQ_LIST.get().unwrap().get(interrupt as usize).unwrap();
        for callback_function in irq_line.callback_list().iter() {
            callback_function.call(trap_frame);
        }
        plic.complete(context_id, interrupt);
    }

    IN_INTERRUPT_CONTEXT.store(false, Ordering::Release);
}

cpu_local! {
    static IN_INTERRUPT_CONTEXT: AtomicBool = AtomicBool::new(false);
}

/// Returns whether we are in the interrupt context.
///
/// FIXME: Here only hardware irq is taken into account. According to linux implementation, if
/// we are in softirq context, or bottom half is disabled, this function also returns true.
pub fn in_interrupt_context() -> bool {
    IN_INTERRUPT_CONTEXT.load(Ordering::Acquire)
}
