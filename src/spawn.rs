//! Execute the game's own item-give routine from outside the process.
//!
//! The Cheat Engine table does this with a code cave plus `CREATETHREAD`: it
//! fills a 0x50-byte item buffer and calls the AOB-scanned `AddItemFunc` with
//! `(inventory_manager, &buffer+0x20, &buffer, 0)`. Linux has no
//! `CreateRemoteThread`, so eldencode borrows the game's main thread with
//! ptrace instead: registers are saved, RIP is pointed straight at that
//! function with the same arguments, and a `int3` byte in the module is used as
//! the return address so the thread traps the moment the call returns. Registers
//! and scratch memory are restored before the thread is released, so the game
//! resumes exactly where it was.
//!
//! No code is ever written into executable memory: the call runs on the game's
//! own code and only the stack area below RSP is borrowed for the item buffer.

use std::io;
use std::time::{Duration, Instant};

use crate::aob::{self, Pattern};
use crate::mem::Memory;
use crate::process::{self, GameProcess};

/// Scratch window reserved just below the thread's stack pointer.
const SCRATCH: u64 = 0x1000;
/// The item struct lives above the callee's stack frame, below the real RSP.
const BUFFER_AT: u64 = 0x100;
/// Where the borrowed return address sits (stack grows down from here).
const STACK_TOP: u64 = 0x800;

pub struct CallResult {
    /// RAX as returned by the game routine.
    pub returned: u64,
}

struct Attachment {
    tid: i32,
}

impl Drop for Attachment {
    fn drop(&mut self) {
        // Signal 0: never deliver a pending fault to the game thread.
        unsafe {
            libc::ptrace(libc::PTRACE_DETACH, self.tid, 0, 0);
        }
    }
}

fn ptrace_error(what: &str) -> String {
    format!("{what}: {}", io::Error::last_os_error())
}

fn attach(tid: i32) -> Result<Attachment, String> {
    if unsafe { libc::ptrace(libc::PTRACE_ATTACH, tid, 0, 0) } < 0 {
        return Err(ptrace_error("ptrace attach failed"));
    }
    let mut status = 0;
    if unsafe { libc::waitpid(tid, &mut status, 0) } < 0 {
        return Err(ptrace_error("waitpid failed"));
    }
    Ok(Attachment { tid })
}

fn get_regs(tid: i32) -> Result<libc::user_regs_struct, String> {
    let mut regs: libc::user_regs_struct = unsafe { std::mem::zeroed() };
    let res = unsafe {
        libc::ptrace(
            libc::PTRACE_GETREGS,
            tid,
            0,
            &mut regs as *mut libc::user_regs_struct as *mut libc::c_void,
        )
    };
    if res < 0 {
        return Err(ptrace_error("ptrace getregs failed"));
    }
    Ok(regs)
}

fn set_regs(tid: i32, regs: &libc::user_regs_struct) -> Result<(), String> {
    let res = unsafe {
        libc::ptrace(
            libc::PTRACE_SETREGS,
            tid,
            0,
            regs as *const libc::user_regs_struct as *mut libc::c_void,
        )
    };
    if res < 0 {
        return Err(ptrace_error("ptrace setregs failed"));
    }
    Ok(())
}

/// x86-64 extended state (SSE/AVX registers plus MXCSR and x87 state). The
/// general-purpose registers alone are not enough: the game runs its render and
/// math code with live XMM values, so a call that clobbers them has to put them
/// back exactly.
const NT_X86_XSTATE: libc::c_uint = 0x202;

fn get_xstate(tid: i32) -> Option<Vec<u8>> {
    // Anything up to AVX-512 fits in 0x4000; the kernel returns the real size.
    let mut buffer = vec![0u8; 0x4000];
    let mut iov = libc::iovec {
        iov_base: buffer.as_mut_ptr().cast(),
        iov_len: buffer.len(),
    };
    let res = unsafe {
        libc::ptrace(
            libc::PTRACE_GETREGSET,
            tid,
            NT_X86_XSTATE as *mut libc::c_void,
            &mut iov as *mut libc::iovec as *mut libc::c_void,
        )
    };
    if res < 0 {
        return None;
    }
    buffer.truncate(iov.iov_len);
    Some(buffer)
}

fn set_xstate(tid: i32, buffer: &[u8]) {
    let mut iov = libc::iovec {
        iov_base: buffer.as_ptr() as *mut libc::c_void,
        iov_len: buffer.len(),
    };
    unsafe {
        libc::ptrace(
            libc::PTRACE_SETREGSET,
            tid,
            NT_X86_XSTATE as *mut libc::c_void,
            &mut iov as *mut libc::iovec as *mut libc::c_void,
        );
    }
}

/// Wait for the borrowed thread to stop (or exit) within `millis`.
fn wait_stop(tid: i32, millis: u64) -> Result<Option<i32>, String> {
    let deadline = Instant::now() + Duration::from_millis(millis);
    loop {
        let mut status = 0;
        let pid = unsafe { libc::waitpid(tid, &mut status, libc::WNOHANG) };
        if pid == tid {
            if libc::WIFSTOPPED(status) {
                return Ok(Some(libc::WSTOPSIG(status)));
            }
            return Ok(None);
        }
        if pid < 0 {
            return Err(ptrace_error("waitpid failed"));
        }
        if Instant::now() > deadline {
            return Err("the game did not return from the item call in time".to_string());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn covers(regions: &[(u64, u64)], start: u64, len: u64) -> bool {
    let end = start.saturating_add(len);
    regions.iter().any(|(a, b)| *a <= start && *b >= end)
}

/// Find a single `int3` byte inside the module's executable pages. It is used as
/// the return address: when the game's routine returns, the thread traps there.
fn trap_gadget(mem: &Memory, proc: &GameProcess) -> Result<u64, String> {
    let pattern = Pattern::parse("CC")?;
    let regions = process::image_exec_regions(proc);
    aob::find(mem, &regions, &pattern, 1)
        .first()
        .copied()
        .ok_or_else(|| "no int3 byte found in the executable image".to_string())
}

/// Call `add_item(inventory, &buffer+0x20, &buffer, 0)` on the game's main
/// thread and return its RAX.
pub fn call_add_item(
    mem: &Memory,
    proc: &GameProcess,
    add_item: u64,
    inventory: u64,
    buffer: &[u8; 0x50],
) -> Result<CallResult, String> {
    let tid = proc.pid;
    let trap = trap_gadget(mem, proc)?;

    let attach = attach(tid)?;
    let original = get_regs(tid)?;
    let xstate = get_xstate(tid);
    let base = (original.rsp & !0xF)
        .checked_sub(SCRATCH)
        .ok_or_else(|| "stack pointer is too low to borrow".to_string())?;

    let writable = process::regions(proc.pid);
    if !covers(&writable, base, SCRATCH) {
        return Err("no writable stack area to borrow for the item buffer".to_string());
    }

    let mut saved = vec![0u8; SCRATCH as usize];
    mem.read(base, &mut saved)
        .map_err(|err| format!("cannot snapshot the stack area: {err}"))?;

    let buffer_at = base + BUFFER_AT;
    mem.write(buffer_at, buffer)
        .map_err(|err| format!("cannot write the item buffer: {err}"))?;
    let return_at = base + STACK_TOP - 8;
    mem.write(return_at, &trap.to_le_bytes())
        .map_err(|err| format!("cannot write the return address: {err}"))?;

    let mut regs = original;
    regs.rip = add_item;
    regs.rsp = return_at;
    // Windows x64: rcx, rdx, r8, r9 — the same arguments the table's stub builds.
    regs.rcx = inventory;
    regs.rdx = buffer_at + 0x20;
    regs.r8 = buffer_at;
    regs.r9 = 0;
    set_regs(tid, &regs)?;

    if unsafe { libc::ptrace(libc::PTRACE_CONT, tid, 0, 0) } < 0 {
        return Err(ptrace_error("ptrace cont failed"));
    }

    let outcome = wait_stop(tid, 2500);
    // Always put the thread back the way we found it, whatever happened.
    let current = get_regs(tid).unwrap_or(original);
    let returned = current.rax;
    if let Some(xstate) = &xstate {
        set_xstate(tid, xstate);
    }
    let _ = set_regs(tid, &original);
    let _ = mem.write(base, &saved);
    drop(attach);

    match outcome {
        Ok(Some(signal)) if signal == libc::SIGTRAP => Ok(CallResult { returned }),
        Ok(Some(signal)) => Err(format!(
            "the item routine faulted (signal {signal}); nothing was added"
        )),
        Ok(None) => Err("the game thread exited during the item call".to_string()),
        Err(err) => Err(err),
    }
}
