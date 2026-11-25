
use std::io;
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::{Duration, Instant};

use crate::aob::{self, Pattern};
use crate::mem::Memory;
use crate::process::{self, GameProcess};

const SCRATCH: u64 = 0x1000;

const BUFFER_AT: u64 = STACK_TOP + 0x100;

const STACK_TOP: u64 = 0x800;

struct Attachment {
    tid: i32,
}

impl Drop for Attachment {
    fn drop(&mut self) {

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

const NT_X86_XSTATE: libc::c_uint = 0x202;

fn get_xstate(tid: i32) -> Option<Vec<u8>> {

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

fn trap_gadget(mem: &Memory, proc: &GameProcess) -> Result<u64, String> {
    let pattern = Pattern::parse("CC")?;
    let regions = process::image_exec_regions(proc);
    aob::find(mem, &regions, &pattern, 1)
        .first()
        .copied()
        .ok_or_else(|| "no int3 byte found in the executable image".to_string())
}

pub struct Spec<'a> {

    pub buffer: Option<(&'a [u8], u64)>,

    pub stack_args: &'a [u64],

    pub capture: Option<(u64, usize)>,

    pub args: &'a dyn Fn(u64) -> [u64; 4],
}

pub struct Outcome {
    pub rax: u64,

    #[allow(dead_code)]
    pub captured: Option<Vec<u8>>,
}

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

static LAST_TID: AtomicI32 = AtomicI32::new(0);

fn parked_threads(proc: &GameProcess) -> Vec<i32> {
    let pid = proc.pid;

    if let Some(forced) = std::env::var("ELDENCODE_TID").ok().and_then(|it| it.parse().ok()) {
        return vec![forced];
    }
    let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return Vec::new();
    };

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

pub fn callable_thread(proc: &GameProcess) -> Option<i32> {
    parked_threads(proc).into_iter().next()
}

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

    if original.gs_base == 0 {
        return Err("no teb: this thread has no Windows TEB".to_string());
    }

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

    regs.rax = 0;
    regs.orig_rax = u64::MAX;

    regs.rcx = rcx;
    regs.rdx = rdx;
    regs.r8 = r8;
    regs.r9 = r9;
    set_regs(tid, &regs)?;

    if unsafe { libc::ptrace(libc::PTRACE_CONT, tid, 0, 0) } < 0 {
        return Err(ptrace_error("ptrace cont failed"));
    }

    let outcome = wait_stop(tid, 10_000).or_else(|err| wait_stop(tid, 20_000).map_err(|_| err));

    let settled = !matches!(outcome, Err(_));

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

        let mut restore = original;
        let code = restore.rax as i64;
        if (-516..=-512).contains(&code) {
            log(&format!(
                "tid {tid}: restoring an interrupted syscall as EINTR (was {code})"
            ));
            restore.rax = (-4i64) as u64;
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
