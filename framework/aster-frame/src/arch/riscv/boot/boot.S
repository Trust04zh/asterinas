/* SPDX-License-Identifier: MPL-2.0 */

.section .text.entry
.globl _start
_start:
    # a0 = hart id
    # a1 = device tree paddr (not touched)

    # 1. set sp
    # sp = boot_stack_top - hart_id * BOOT_STACK_HART_SIZE
    slli   t0, a0, {BOOT_STACK_HART_SHIFT}
    lga    sp, boot_stack_top
    sub    sp, sp, t0

    # 2. enable paging
    # setting up 1st pagetable
    # entry = (PPN(boot_pagetable_2nd) << 10) | 0x01 # V
    la     t0, boot_pagetable_2nd
    srli   t0, t0, 2
    ori    t0, t0, 0x01
    la     t1, boot_pagetable + 8 * 128
    sd     t0, (-8 * 128)(t1)
    sd     t0, (8 * 128)(t1)
    # satp = (8 << 60) | PPN(boot_pagetable)
    la     t0, boot_pagetable
    li     t1, 9 << 60
    srli   t0, t0, 12
    or     t0, t0, t1
    csrw   satp, t0
    sfence.vma

    # 3. jump to rust riscv_boot
    lga    t0, riscv_boot
    jr     t0


.section .bss.stack

.globl boot_stack_lower_bound
boot_stack_lower_bound:
    .space {BOOT_STACK_SIZE}

.globl boot_stack_top
boot_stack_top:


.section .data

.align 12
boot_pagetable:
    # 0x0000_0000_0000_0000 -> second_table
    # 0xffff_8000_0000_0000 -> second_table
    .zero 8 * 512

boot_pagetable_2nd:
    # 0x0000_0000_8000_0000 -> 0x0000_0000_8000_0000
    .quad (0x00000 << 10) | 0xcf # VRWXAD
    .quad (0x40000 << 10) | 0xcf # VRWXAD
    .quad (0x80000 << 10) | 0xcf # VRWXAD
    .zero 8 * 509
