core::arch::global_asm!(include_str!("switch.S"));

#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct CalleeRegs {
    pub sp: u64,
    pub s0: u64,
    pub s1: u64,
    pub s2: u64,
    pub s3: u64,
    pub s4: u64,
    pub s5: u64,
    pub s6: u64,
    pub s7: u64,
    pub s8: u64,
    pub s9: u64,
    pub s10: u64,
    pub s11: u64,
    // floating point?
}

#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub(crate) struct TaskContext {
    pub regs: CalleeRegs,
    pub pc: usize,
}

impl TaskContext {
    pub fn get_sp(&self) -> u64 {
        self.regs.sp
    }

    pub fn set_sp(&mut self, sp: u64) {
        self.regs.sp = sp;
    }

    pub fn get_pc(&self) -> usize {
        self.pc
    }

    pub fn set_pc(&mut self, pc: usize) {
        self.pc = pc;
    }
}

extern "C" {
    pub(crate) fn context_switch(cur: *mut TaskContext, nxt: *const TaskContext);
}
