// SPDX-License-Identifier: MPL-2.0

use super::{SyscallReturn, SYS_GETPGID};
use crate::{
    log_syscall_entry,
    prelude::*,
    process::{process_table, Pgid, Pid},
};

pub fn sys_getpgid(pid: Pid) -> Result<SyscallReturn> {
    log_syscall_entry!(SYS_GETPGID);
    let current = current!();
    // if pid is 0, pid should be the pid of current process
    let pid = if pid == 0 { current.pid() } else { pid };

    let process = process_table::get_process(&pid)
        .ok_or(Error::with_message(Errno::ESRCH, "process does not exist"))?;

    process.pgid();

    Ok(SyscallReturn::Return(process.pgid() as _))
}
