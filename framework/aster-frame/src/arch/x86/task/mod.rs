core::arch::global_asm!(include_str!("switch.S"));

#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub struct CalleeRegs {
    pub rsp: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
}

#[derive(Debug, Default, Clone, Copy)]
#[repr(C)]
pub(crate) struct TaskContext {
    pub regs: CalleeRegs,
    pub rip: usize,
}

impl TaskContext {
    pub fn get_sp(&self) -> u64 {
        self.regs.rsp
    }

    pub fn set_sp(&mut self, sp: u64) {
        self.regs.rsp = sp;
    }

    pub fn get_pc(&self) -> usize {
        self.rip
    }

    pub fn set_pc(&mut self, pc: usize) {
        self.rip = pc;
    }
}

extern "C" {
    pub(crate) fn context_switch(cur: *mut TaskContext, nxt: *const TaskContext);
}
