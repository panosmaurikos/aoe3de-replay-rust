//! Windows backend: attach to the running game and read its memory **externally**
//! (`OpenProcess` + `ReadProcessMemory`) — no injection, no code run in the game.
//! Module base is found via the ToolHelp snapshot API.

use super::ProcessMemory;
use super::config::OffsetConfig;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
    MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};

/// A handle to the live game process plus the resolved module base.
pub struct WindowsProcess {
    handle: HANDLE,
    base: u64,
}

impl WindowsProcess {
    /// Find `cfg.process_name`, open it read-only, and resolve `cfg.module_name`'s
    /// base address.
    pub fn attach(cfg: &OffsetConfig) -> Result<Self, String> {
        let pid = find_process_id(&cfg.process_name).ok_or_else(|| {
            format!(
                "process '{}' not found — start AoE3 DE and load a game/replay first",
                cfg.process_name
            )
        })?;

        // SAFETY: standard Win32 call; handle validity is checked below.
        let handle =
            unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, 0, pid) };
        if handle.is_null() {
            return Err(format!(
                "OpenProcess failed for pid {pid} (run this tool as the same user; \
                 try an elevated terminal if needed)"
            ));
        }

        let base = match module_base(pid, &cfg.module_name) {
            Some(b) => b,
            None => {
                // SAFETY: handle is valid here.
                unsafe { CloseHandle(handle) };
                return Err(format!(
                    "module '{}' not found in process {pid}",
                    cfg.module_name
                ));
            }
        };

        Ok(Self { handle, base })
    }
}

impl Drop for WindowsProcess {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: handle was opened in `attach` and not closed elsewhere.
            unsafe { CloseHandle(self.handle) };
        }
    }
}

impl ProcessMemory for WindowsProcess {
    fn module_base(&self) -> u64 {
        self.base
    }

    fn read_bytes(&self, addr: u64, len: usize) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; len];
        let mut read: usize = 0;
        // SAFETY: buf is `len` bytes; ReadProcessMemory writes at most `len`.
        let ok = unsafe {
            ReadProcessMemory(
                self.handle,
                addr as *const core::ffi::c_void,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                len,
                &mut read,
            )
        };
        if ok == 0 || read != len {
            return Err(format!(
                "ReadProcessMemory failed at {addr:#x} (read {read}/{len}) — \
                 stale pointer chain or the match ended"
            ));
        }
        Ok(buf)
    }
}

/// UTF-16 NUL-terminated array → String.
fn wide_to_string(wide: &[u16]) -> String {
    let end = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
    String::from_utf16_lossy(&wide[..end])
}

/// Find the first process whose exe name matches `name` (case-insensitive).
fn find_process_id(name: &str) -> Option<u32> {
    // SAFETY: ToolHelp snapshot APIs; the snapshot handle is closed before return.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = core::mem::zeroed();
        entry.dwSize = core::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let exe = wide_to_string(&entry.szExeFile);
                if exe.eq_ignore_ascii_case(name) {
                    found = Some(entry.th32ProcessID);
                    break;
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        found
    }
}

/// Base load address of `module_name` within `pid` (case-insensitive).
fn module_base(pid: u32, module_name: &str) -> Option<u64> {
    // SAFETY: ToolHelp module snapshot; handle closed before return.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: MODULEENTRY32W = core::mem::zeroed();
        entry.dwSize = core::mem::size_of::<MODULEENTRY32W>() as u32;
        let mut base = None;
        if Module32FirstW(snap, &mut entry) != 0 {
            loop {
                let name = wide_to_string(&entry.szModule);
                if name.eq_ignore_ascii_case(module_name) {
                    base = Some(entry.modBaseAddr as u64);
                    break;
                }
                if Module32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        base
    }
}
