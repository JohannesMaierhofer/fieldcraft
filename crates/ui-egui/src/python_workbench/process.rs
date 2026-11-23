//! Desktop subprocesses with a shared log and a nonblocking console input queue.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, SyncSender};

const MAX_LINE: usize = 16 * 1024;

pub(super) struct Process {
    child: Option<Child>,
    input: Option<SyncSender<Vec<u8>>>,
    log: File,
    pending: Vec<u8>,
    status: Option<ExitStatus>,
}

impl Process {
    pub(super) fn spawn(command: &mut Command, path: &Path, console: bool) -> io::Result<Self> {
        let output = OpenOptions::new().append(true).create_new(true).open(path)?;
        let log = File::open(path)?;
        command.stdout(Stdio::from(output.try_clone()?)).stderr(Stdio::from(output)).stdin(if console { Stdio::piped() } else { Stdio::null() });
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(command, 0);
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(command, 0x0800_0000);
        let mut child = command.spawn()?;
        let stdin = child.stdin.take();
        let mut process = Self { child: Some(child), input: None, log, pending: Vec::new(), status: None };
        if let Some(mut stdin) = stdin {
            #[allow(clippy::disallowed_methods, reason = "the input worker receives and the window only calls try_send")]
            let (send, receive) = mpsc::sync_channel::<Vec<u8>>(1);
            std::thread::Builder::new().name("fieldcraft-python-input".into()).spawn(move || {
                #[allow(clippy::disallowed_methods, reason = "only the input worker waits for requests")]
                while let Ok(bytes) = receive.recv() {
                    if stdin.write_all(&bytes).is_err() {
                        break;
                    }
                }
            })?;
            process.input = Some(send);
        }
        Ok(process)
    }

    pub(super) fn send(&self, mut bytes: Vec<u8>) -> Result<(), String> {
        bytes.push(b'\n');
        self.input.as_ref().ok_or("Python console input is closed")?.try_send(bytes).map_err(|error| error.to_string())
    }

    /// Read at most 32 KiB per frame. Very long lines are split. The saved log retains the full output.
    pub(super) fn poll(&mut self, mut line: impl FnMut(String)) -> io::Result<Option<ExitStatus>> {
        if self.status.is_none()
            && let Some(child) = &mut self.child
        {
            self.status = child.try_wait()?;
        }
        let mut bytes = [0; 32 * 1024];
        let count = self.log.read(&mut bytes)?;
        for byte in bytes.into_iter().take(count) {
            if byte == b'\n' || self.pending.len() == MAX_LINE {
                line(String::from_utf8_lossy(&self.pending).trim_end_matches('\r').to_string());
                self.pending.clear();
            }
            if byte != b'\n' {
                self.pending.push(byte);
            }
        }
        if count == 0 && self.status.is_some() && !self.pending.is_empty() {
            line(String::from_utf8_lossy(&self.pending).into_owned());
            self.pending.clear();
        }
        Ok(self.status.filter(|_| count == 0))
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        self.input.take();
        if self.status.is_some() {
            return;
        }
        let Some(mut child) = self.child.take() else { return };
        let pid = child.id();
        #[cfg(unix)]
        let _ = Command::new("/bin/kill").args(["-KILL", "--", &format!("-{pid}")]).stderr(Stdio::null()).status();
        #[cfg(windows)]
        let _ = std::env::var_os("SystemRoot").map(|root| {
            use std::os::windows::process::CommandExt;
            Command::new(std::path::PathBuf::from(root).join("System32/taskkill.exe"))
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(0x0800_0000)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
        });
        let _ = child.kill();
        let _ = std::thread::Builder::new().name("fieldcraft-python-reaper".into()).spawn(move || child.wait());
    }
}
