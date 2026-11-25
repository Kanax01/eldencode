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
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use crate::aob::{self, Pattern};
use crate::mem::Memory;
use crate::process::{self, GameProcess};

/// Scratch window reserved just below the thread's stack pointer.
const SCRATCH: u64 = 0x1000;
/// Where a call's data buffer lives: above the borrowed return address, between
/// it and the thread's real RSP.
///
/// This is not cosmetic. The callee's frame grows *down* from the return
/// address, so anything placed below it is the routine's own stack and is
/// overwritten by the calls it makes. An item struct parked there came back as a
/// garbage id and quantity — which is what turned a give into a wedged or dead
/// game. Above the return address the callee can never reach.
const BUFFER_AT: u64 = STACK_TOP + 0x100;
/// Where the borrowed return address sits (stack grows down from here).
const STACK_TOP: u64 = 0x800;

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

fn set_xstate(tid: i32, buffer: &[u8]) -> bool {
    let mut iov = libc::iovec {
        iov_base: buffer.as_ptr() as *mut libc::c_void,
        iov_len: buffer.len(),
    };
    let res = unsafe {
        libc::ptrace(
            libc::PTRACE_SETREGSET,
            tid,
            NT_X86_XSTATE as *mut libc::c_void,
            &mut iov as *mut libc::iovec as *mut libc::c_void,
        )
    };
    res >= 0
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

/// Address the faulting access hit (`si_addr`), for the log.
fn fault_address(tid: i32) -> u64 {
    let mut info = [0u8; 128];
    let res = unsafe {
        libc::ptrace(
            libc::PTRACE_GETSIGINFO,
            tid,
            0,
            info.as_mut_ptr() as *mut libc::c_void,
        )
    };
    if res < 0 || info.len() < 24 {
        return 0;
    }
    u64::from_ne_bytes(info[16..24].try_into().unwrap_or([0; 8]))
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

/// Call a game routine on the main thread and return its RAX.
///
/// `bytes` is written into the borrowed buffer; `args` receives that buffer's
/// address and returns the four register arguments (rcx, rdx, r8, r9) — the
/// Windows x64 convention the game uses.
/// Everything `run` needs: the bytes to place in the borrowed area, the two
/// stack arguments x64 uses for a 5th and 6th parameter, and the registers.
pub struct Spec<'a> {
    /// Bytes written at `offset` in the borrowed area.
    pub buffer: Option<(&'a [u8], u64)>,
    /// Arguments 5 and 6, which Win64 passes on the stack.
    pub stack_args: &'a [u64],
    /// `(offset, len)` read back out of the area before it is restored, for
    /// calls that return a value through a pointer.
    pub capture: Option<(u64, usize)>,
    /// Builds rcx, rdx, r8, r9 from the address of `buffer`.
    pub args: &'a dyn Fn(u64) -> [u64; 4],
}

pub struct Outcome {
    pub rax: u64,
    /// Value a call writes back through a pointer, when a `Spec` asks for it.
    #[allow(dead_code)]
    pub captured: Option<Vec<u8>>,
}

/// State letter of a thread (`R`, `S`, `D`, …) from /proc/<tid>/stat.
/// Append a line to `$HOME/.local/share/eldencode/call.log`, and to stderr when
/// `ELDENCODE_TRACE` is set.
///
/// Every borrowed-thread call records which thread it picked, what that thread
/// was waiting on, how long the game took to return and whether it returned at
/// all — so a freeze leaves evidence instead of a guess.
pub fn log(msg: &str) {
    use std::io::Write;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Some(home) = std::env::var_os("HOME") {
        let path = std::path::PathBuf::from(home).join(".local/share/eldencode/call.log");
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(file, "{stamp} {msg}");
        }
    }
    if std::env::var_os("ELDENCODE_TRACE").is_some() {
        eprintln!("{stamp} {msg}");
    }
}

/// What a thread is waiting in, when it is parked (`ep_poll`, `futex`, …).
fn thread_wchan(tid: i32) -> String {
    std::fs::read_to_string(format!("/proc/{tid}/wchan"))
        .map(|it| it.trim().to_string())
        .unwrap_or_default()
}

fn thread_state(tid: i32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{tid}/stat")).ok()?;
    let rest = stat.rsplit_once(')')?.1;
    rest.split_whitespace().next()?.chars().next()
}

/// Threads a game routine can be run from, best first.
///
/// Under Proton (Wine + DXVK) every Windows thread of `eldenring.exe` is a Wine
/// thread, but not every Wine thread is a Windows one: Wine also runs unix-side
/// helper threads, and those have no TEB at all. Redirecting one of them into
/// game code faults straight away on `gs:[0x30]` — the TEB self pointer — which
/// reads address 0x30. `run` filters them out by asking each thread for its GS
/// base after attaching; the order below is only a preference.
/// The thread that last took a call, reused by the next one: a bulk script then
/// does one attach per item instead of a sweep over every parked thread.
static LAST_TID: AtomicI32 = AtomicI32::new(0);

fn parked_threads(proc: &GameProcess) -> Vec<i32> {
    let pid = proc.pid;
    // `ELDENCODE_TID=<tid>` forces one thread: used to find out which of the
    // game's threads can run its routines at all.
    if let Some(forced) = std::env::var("ELDENCODE_TID").ok().and_then(|it| it.parse().ok()) {
        return vec![forced];
    }
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return Vec::new();
    };
    // Tier 0 is a thread parked in an event wait: no lock in hand, nothing in
    // flight. Tier 1 is any other sleep, tier 2 a thread waiting on a futex
    // (which may be waiting *inside* the allocator). `D` is a kernel wait and is
    // never used.
    let mut all: Vec<(u8, i32)> = Vec::new();
    for entry in entries.flatten() {
        let Ok(tid) = entry.file_name().to_string_lossy().parse::<i32>() else {
            continue;
        };
        if tid == pid || thread_state(tid) != Some('S') {
            continue;
        }
        let tier = match thread_wchan(tid).as_str() {
            "ep_poll" | "poll" | "ppoll" | "select" | "do_select" | "nanosleep"
            | "do_nanosleep" | "hrtimer_nanosleep" => 0,
            "futex" => 2,
            _ => 1,
        };
        all.push((tier, tid));
    }
    all.sort();
    let mut tids: Vec<i32> = all.into_iter().map(|(_, tid)| tid).collect();
    let last = LAST_TID.load(Ordering::Relaxed);
    if last != 0 && tids.contains(&last) {
        tids.retain(|tid| *tid != last);
        tids.insert(0, last);
    }
    tids
}

/// Best candidate, for reporting.
pub fn callable_thread(proc: &GameProcess) -> Option<i32> {
    parked_threads(proc).into_iter().next()
}

/// Call a routine inside the game from one of its threads.
///
/// The game's own thread is pointed at `target` with a return address that
/// points at an `int3`, its registers and vector state are saved first and put
/// back afterwards. Only the small area below RSP is borrowed, and it is
/// snapshotted and restored, so nothing the game was using is disturbed.
pub fn run(
    mem: &Memory,
    proc: &GameProcess,
    target: u64,
    spec: &Spec<'_>,
) -> Result<Outcome, String> {
    let candidates = parked_threads(proc);
    if candidates.is_empty() {
        log("no parked game thread available");
        return Err(
            "no parked game thread to run the item call on — try again once the game is idle"
                .to_string(),
        );
    }
    log(&format!(
        "call {target:#X}: {} parked threads to try",
        candidates.len()
    ));
    let mut last = String::from("no thread with a Windows TEB was found");
    let mut without_teb = 0usize;
    for tid in candidates {
        let started = Instant::now();
        let outcome = run_on(mem, proc, tid, target, spec);
        match outcome {
            Ok(outcome) => {
                LAST_TID.store(tid, Ordering::Relaxed);
                log(&format!(
                    "call {target:#X} on tid {tid}: returned, rax {:#X} after {} ms \
                     ({without_teb} threads without a TEB skipped)",
                    outcome.rax,
                    started.elapsed().as_millis()
                ));
                return Ok(outcome);
            }
            // Nothing ran on that thread: it has no TEB, so it is safe to try
            // the next one. A fault is different — the routine got somewhere,
            // so another thread could make it worse.
            Err(err) if err.starts_with("no teb") => {
                without_teb += 1;
                last = err;
                continue;
            }
            Err(err) => {
                log(&format!(
                    "call {target:#X} on tid {tid}: failed: {err} after {} ms",
                    started.elapsed().as_millis()
                ));
                return Err(err);
            }
        }
    }
    log(&format!(
        "call {target:#X}: {without_teb} parked threads, none with a Windows TEB"
    ));
    Err(last)
}

fn run_on(
    mem: &Memory,
    proc: &GameProcess,
    tid: i32,
    target: u64,
    spec: &Spec<'_>,
) -> Result<Outcome, String> {
    let trap = trap_gadget(mem, proc)?;

    let attach = attach(tid)?;
    let original = get_regs(tid)?;
    // Wine's unix-side threads have no TEB, and game code reads it through
    // `gs:[0x30]`; running there faults on address 0x30 immediately.
    if original.gs_base == 0 {
        return Err("no teb: this thread has no Windows TEB".to_string());
    }
    // Without the vector state there is nothing safe to restore, so refuse
    // rather than leaving the game's XMM/ZMM registers clobbered.
    let xstate = get_xstate(tid).ok_or_else(|| {
        "cannot save the game thread's vector state (PTRACE_GETREGSET); refusing to call"
            .to_string()
    })?;
    let base = (original.rsp & !0xF)
        .checked_sub(SCRATCH)
        .ok_or_else(|| "stack pointer is too low to borrow".to_string())?;

    let writable = process::regions(proc.pid);
    if !covers(&writable, base, SCRATCH) {
        return Err("no writable stack area to borrow for the call".to_string());
    }

    let mut saved = vec![0u8; SCRATCH as usize];
    mem.read(base, &mut saved)
        .map_err(|err| format!("cannot snapshot the stack area: {err}"))?;

    let buffer_at = match spec.buffer {
        Some((bytes, offset)) => {
            let at = base + offset;
            mem.write(at, bytes)
                .map_err(|err| format!("cannot write the call buffer: {err}"))?;
            at
        }
        None => base,
    };
    let return_at = base + STACK_TOP - 8;
    mem.write(return_at, &trap.to_le_bytes())
        .map_err(|err| format!("cannot write the return address: {err}"))?;
    for (i, value) in spec.stack_args.iter().enumerate() {
        mem.write(return_at + 0x20 + i as u64 * 8, &value.to_le_bytes())
            .map_err(|err| format!("cannot write the stack argument: {err}"))?;
    }

    let [rcx, rdx, r8, r9] = (spec.args)(buffer_at);
    log(&format!(
        "tid {tid} at attach: rip {:#X} rsp {:#X} rax {:#X} orig_rax {:#X} gs {:#X}",
        original.rip, original.rsp, original.rax, original.orig_rax, original.gs_base
    ));
    let mut regs = original;
    regs.rip = target;
    regs.rsp = return_at;
    // Every wine thread is parked inside a syscall, and an interrupted one is
    // resumed with `rax = -ERESTARTSYS`: the kernel then rewinds RIP by the
    // length of the syscall instruction, which lands two bytes before the
    // function we just pointed the thread at. Report success instead, so the
    // thread starts cleanly at the routine.
    regs.rax = 0;
    regs.orig_rax = u64::MAX;
    // Windows x64: rcx, rdx, r8, r9 — the same arguments the table's stubs build.
    regs.rcx = rcx;
    regs.rdx = rdx;
    regs.r8 = r8;
    regs.r9 = r9;
    set_regs(tid, &regs)?;

    if unsafe { libc::ptrace(libc::PTRACE_CONT, tid, 0, 0) } < 0 {
        return Err(ptrace_error("ptrace cont failed"));
    }

    // A give can block on the game's own work, and the first call after an
    // attach is the slowest. Pulling the thread out mid-call corrupts it, so
    // wait properly before concluding anything.
    let outcome = wait_stop(tid, 10_000).or_else(|err| wait_stop(tid, 20_000).map_err(|_| err));
    // Restoring the registers and the stack under a call that has not returned
    // would hand the thread back with someone else's frame: leave it alone and
    // report instead.
    let settled = !matches!(outcome, Err(_));
    // Always put the thread back the way we found it, whatever happened.
    let current = if settled {
        get_regs(tid).unwrap_or(original)
    } else {
        original
    };
    let returned = current.rax;
    let captured = spec.capture.and_then(|(offset, len)| {
        let mut bytes = vec![0u8; len];
        mem.read(base + offset, &mut bytes).ok().map(|_| bytes)
    });
    // Record where a fault happened before detaching: after the detach there is
    // no siginfo left to ask for.
    let fault = match outcome {
        Ok(Some(signal)) if signal != libc::SIGTRAP => Some((
            signal,
            current.rip,
            current.rsp,
            fault_address(tid),
        )),
        _ => None,
    };
    let xstate_ok = !settled || set_xstate(tid, &xstate);
    if settled {
        // The thread was interrupted inside a syscall (`futex`, rax =
        // -ERESTART_RESTARTBLOCK). By the time we put it back, the kernel has
        // already unwound that restart, so -5xx is not a value user space can
        // interpret: Wine treats it as a bogus errno and spins on it, and the
        // game's own loop starves (picture frozen, sound still playing).
        // Report the interrupt instead, and its wait loop simply re-arms.
        let mut restore = original;
        let code = restore.rax as i64;
        if (-516..=-512).contains(&code) {
            log(&format!(
                "tid {tid}: restoring an interrupted syscall as EINTR (was {code})"
            ));
            restore.rax = (-4i64) as u64; // -EINTR
        }
        let _ = set_regs(tid, &restore);
        let _ = mem.write(base, &saved);
    }
    drop(attach);

    match outcome {
        Ok(Some(signal)) if signal == libc::SIGTRAP => {
            if !xstate_ok {
                return Err(
                    "the game thread's vector state could not be restored — restart the game"
                        .to_string(),
                );
            }
            Ok(Outcome {
                rax: returned,
                captured,
            })
        }
        Ok(Some(signal)) => {
            let (_, rip, rsp, addr) = fault.unwrap_or((signal, 0, 0, 0));
            log(&format!(
                "fault: signal {signal} at rip {rip:#X} rsp {rsp:#X} addr {addr:#X}"
            ));
            Err(format!(
                "the game routine faulted (signal {signal}); nothing was done"
            ))
        }
        Ok(None) => Err("the game thread exited during the call".to_string()),
        Err(_) => Err(
            "the game is still inside the item call after 30s; it was left alone rather than \
             pulled out mid-call — restart the game"
                .to_string(),
        ),
    }
}

/// Shorthand for a call whose only input is a small buffer in the borrowed area.
pub fn call(
    mem: &Memory,
    proc: &GameProcess,
    target: u64,
    bytes: &[u8],
    args: &dyn Fn(u64) -> [u64; 4],
) -> Result<u64, String> {
    run(
        mem,
        proc,
        target,
        &Spec {
            buffer: Some((bytes, BUFFER_AT)),
            stack_args: &[],
            capture: None,
            args,
        },
    )
    .map(|outcome| outcome.rax)
}

/// Shorthand for a call that only takes register arguments, the way the table's
/// `executeCode`/`executeCodeEx` entries call the game's own routines: no buffer
/// is written, the four arguments go straight into rcx/rdx/r8/r9.
pub fn call_args(
    mem: &Memory,
    proc: &GameProcess,
    target: u64,
    args: [u64; 4],
) -> Result<u64, String> {
    let build = |_: u64| args;
    run(
        mem,
        proc,
        target,
        &Spec {
            buffer: None,
            stack_args: &[],
            capture: None,
            args: &build,
        },
    )
    .map(|outcome| outcome.rax)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The call buffer must not sit inside the callee's own stack frame: the
    /// routine grows down from the borrowed return address and overwrites
    /// everything below it. That is what corrupted item structs.
    #[test]
    fn the_call_buffer_is_out_of_the_callee_frame() {
        assert!(
            BUFFER_AT > STACK_TOP,
            "buffer at {BUFFER_AT:#X} is below the borrowed return address"
        );
        assert!(
            BUFFER_AT + 0x100 <= SCRATCH,
            "buffer would run past the borrowed stack window"
        );
        assert!(
            STACK_TOP + 8 + 0x20 + 16 < BUFFER_AT,
            "the stack arguments would collide with the buffer"
        );
    }
}
