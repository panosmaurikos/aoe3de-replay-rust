//! Non-Windows stub. Live capture reads a running Windows game process, so the
//! real backend only exists on Windows; elsewhere `attach` fails cleanly and the
//! rest of the CLI keeps working.

use super::ProcessMemory;
use super::config::CaptureConfig;

pub struct StubProcess;

impl StubProcess {
    pub fn attach(_cfg: &CaptureConfig) -> Result<Self, String> {
        Err("Mode B live capture is only supported on Windows (it reads the running \
             AoE3DE_s.exe via ReadProcessMemory)"
            .into())
    }
}

impl ProcessMemory for StubProcess {
    fn module_base(&self) -> u64 {
        0
    }
    fn module_size(&self) -> u64 {
        0
    }
    fn read_bytes(&self, _addr: u64, _len: usize) -> Result<Vec<u8>, String> {
        Err("Mode B live capture is only supported on Windows".into())
    }
}
