.intel_syntax noprefix
.code64

.extern rust_syscall_handler

/*
 * This is the code that is run when a task executes the `syscall` instruction. The `syscall`
 * instruction:
 *    - has put the `rip` to return to in `rcx`
 *    - has put the `rflags` to return with in `r11`, masked with `IA32_FMASK`
 *    - does not save `rsp`. It is our responsibility to deal with the stack(s).
 * 
 * Because we are using the System-V ABI:
 *    - `rbp`, `rbx`, `r12`, `r13`, `r14`, and `r15` must be preserved
 *    - Other registers may be clobbered
 * 
 * Values of registers for syscall instructions.
 *     rdi = number
 *     rsi = a
 *     rdx = b
 *     r10 = c
 *     r8  = d
 *     r9  = e
 * This is only different from the Sys-V ABI in that `c` is in `r10` and not `rcx` (because `rcx` is being
 * used by syscall). To call into the Rust function (as long as it is using the C ABI), we only need to
 * move that one parameter.
 * 
 * TODO: I think this leaks some kernel addresses to userspace (such as the task's kernel stack
 * address!). We should probably zero some registers.
 */
.global syscall_handler
syscall_handler:
    // Firstly, we move to the task's kernel stack. `r10` doesn't contain anything important atm.
    mov r10, gs:0x8    // This accesses the `current_task_kernel_rsp` field of the per-cpu data
    xchg r10, rsp
    push r10           // Push the user stack pointer onto the task's kernel stack so we can move back to it later

    // The `syscall` instruction puts important stuff in `rcx` and `r11`, so we save them and restore them
    // before calling `sysretq`.
    push rcx
    push r11

    // Move `c` into the right register. This is fine now because we've saved syscall's expected `rcx` on the
    // stack.
    mov rcx, r11

    // Call the Rust handler
    call rust_syscall_handler

    // Zero registers trashed by the Rust code before we return to userspace
    xor rdi, rdi
    xor rsi, rsi
    xor rdx, rdx
    xor r10, r10
    xor r8, r8
    xor r9, r9

    // Restore state needed for `sysretq`
    pop r11
    pop rcx

    // Move back to task's user stack, saving kernel stack back into per-cpu
    mov gs:0x8, rsp
    pop rsp
    /* pop r10 */
    /* xchg r10, rsp */
    /* mov gs:0x8, r10 */

    // Return to userspace!
    sysretq
