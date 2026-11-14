//! Keep the computer from sleeping while a terminal tab runs a command
//! (Settings → Terminal). Only idle sleep is held off: closing the lid or
//! choosing Sleep still works, and the display may still turn off.
//!
//! - Windows: SetThreadExecutionState on a thread that lives as long as the hold.
//! - macOS: `caffeinate -i -w <our pid>`, which also ends if LiteTerm dies.
//! - Linux: `systemd-inhibit` around a `tail` that ends with LiteTerm.

use std::sync::Mutex;

static HOLD: Mutex<Option<Hold>> = Mutex::new(None);

#[tauri::command]
pub fn set_keep_awake(on: bool) {
    let mut hold = HOLD.lock().unwrap();
    if on && hold.is_none() {
        match Hold::start() {
            Ok(h) => *hold = Some(h),
            Err(e) => crate::applog::write(&format!("keep awake unavailable: {e}")),
        }
    } else if !on {
        // Dropping the hold lets the system sleep again
        *hold = None;
    }
}

#[cfg(windows)]
struct Hold {
    stop: std::sync::mpsc::Sender<()>,
}

#[cfg(windows)]
impl Hold {
    fn start() -> Result<Self, String> {
        use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
        let (stop, wait) = std::sync::mpsc::channel::<()>();
        std::thread::Builder::new()
            .name("keep-awake".into())
            .spawn(move || {
                // The request belongs to this thread and lasts until it is reset
                unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
                let _ = wait.recv();
                unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { stop })
    }
}

#[cfg(windows)]
impl Drop for Hold {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

#[cfg(not(windows))]
struct Hold {
    child: std::process::Child,
}

#[cfg(not(windows))]
impl Hold {
    fn start() -> Result<Self, String> {
        use std::process::{Command, Stdio};
        let pid = std::process::id().to_string();
        let mut cmd;
        #[cfg(target_os = "macos")]
        {
            cmd = Command::new("caffeinate");
            cmd.args(["-i", "-w", pid.as_str()]);
        }
        #[cfg(not(target_os = "macos"))]
        {
            use std::os::unix::process::CommandExt;
            let tail_pid = format!("--pid={pid}");
            cmd = Command::new("systemd-inhibit");
            cmd.args([
                "--what=idle:sleep",
                "--who=LiteTerm",
                "--why=A command is running in a terminal tab",
                "--mode=block",
                "tail",
                tail_pid.as_str(),
                "-f",
                "/dev/null",
            ]);
            // Its own process group, so releasing ends the `tail` as well
            cmd.process_group(0);
        }
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(Self { child })
    }
}

#[cfg(not(windows))]
impl Drop for Hold {
    fn drop(&mut self) {
        #[cfg(not(target_os = "macos"))]
        {
            let group = format!("-{}", self.child.id());
            let _ = std::process::Command::new("kill").args(["-TERM", "--", &group]).status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
