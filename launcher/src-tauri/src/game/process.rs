//! JVM process supervision — the real source behind the Console tab.
//!
//! Spawns the game, pumps stdout+stderr off the child on dedicated threads,
//! parses each line into a structured `LogLine`, streams it to the webview as
//! a `game://log` event, and mirrors everything to disk for crash reports.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    thread,
};
use tauri::{AppHandle, Emitter};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct LogLine {
    pub seq: u64,
    pub ts: String,
    pub thread: String,
    pub level: Level,
    pub msg: String,
}

/// Vanilla and Log4j both emit `[HH:MM:SS] [Thread/LEVEL]: message`.
/// Anything that does not match is still surfaced — as INFO on a `stdout`
/// pseudo-thread — because silently dropping unparsed output is how you lose
/// the one stack trace that mattered.
fn parse(line: &str, from_stderr: bool) -> (String, Level, String) {
    let fallback = || {
        (
            if from_stderr { "stderr" } else { "stdout" }.to_string(),
            if from_stderr { Level::Error } else { Level::Info },
            line.to_string(),
        )
    };

    let Some(rest) = line.strip_prefix('[') else { return fallback() };
    let Some((_time, rest)) = rest.split_once("] [") else { return fallback() };
    let Some((meta, msg)) = rest.split_once("]: ") else { return fallback() };
    let Some((thread, lvl)) = meta.rsplit_once('/') else { return fallback() };

    let level = match lvl.trim().to_ascii_uppercase().as_str() {
        "DEBUG" | "TRACE" => Level::Debug,
        "WARN" => Level::Warn,
        "ERROR" | "FATAL" | "SEVERE" => Level::Error,
        _ => Level::Info,
    };
    (thread.trim().to_string(), level, msg.to_string())
}

fn now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("[{:02}:{:02}:{:02}]", (d / 3600) % 24, (d / 60) % 60, d % 60)
}

pub struct Session {
    child: Mutex<Option<Child>>,
    pub pid: u32,
    seq: AtomicU32,
    running: AtomicBool,
}

impl Session {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Graceful first, forceful second. A modded JVM that ignores the console
    /// close event still has to die, or the next launch hits a locked
    /// `.minecraft` directory and fails with a lock error nobody can diagnose.
    pub fn kill(&self) -> Result<()> {
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            child.kill().context("failed to terminate game process")?;
            let _ = child.wait();
        }
        self.running.store(false, Ordering::Relaxed);
        Ok(())
    }
}

pub fn launch(app: AppHandle, java: &str, args: &[String], cwd: &std::path::Path) -> Result<Arc<Session>> {
    let mut cmd = Command::new(java);
    cmd.args(args)
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    // Without this the JVM pops a console window behind the launcher.
    // The same DWORD also carries the priority class: ABOVE_NORMAL keeps the
    // game ahead of background apps without the OS starvation HIGH/REALTIME
    // can cause. (creation_flags REPLACES, so both flags go in one call.)
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const ABOVE_NORMAL_PRIORITY_CLASS: u32 = 0x0000_8000;
        cmd.creation_flags(CREATE_NO_WINDOW | ABOVE_NORMAL_PRIORITY_CLASS);
    }

    let mut child = cmd.spawn().context("failed to spawn the JVM")?;
    let pid = child.id();
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");

    let session = Arc::new(Session {
        child: Mutex::new(None),
        pid,
        seq: AtomicU32::new(0),
        running: AtomicBool::new(true),
    });

    // Mirror to disk. The in-memory console is a 2000-line ring buffer; a crash
    // report needs the whole session.
    let log_path = crate::paths::log_dir()?.join(format!("session-{pid}.log"));
    let file = Arc::new(Mutex::new(std::fs::File::create(&log_path)?));

    let pump = |reader: Box<dyn BufRead + Send>, from_stderr: bool| {
        let app = app.clone();
        let session = session.clone();
        let file = file.clone();
        thread::spawn(move || {
            for line in reader.lines().map_while(Result::ok) {
                let (thread_name, level, msg) = parse(&line, from_stderr);
                let entry = LogLine {
                    seq: session.seq.fetch_add(1, Ordering::Relaxed) as u64,
                    ts: now(),
                    thread: thread_name,
                    level,
                    msg,
                };
                if let Ok(mut f) = file.lock() {
                    let _ = writeln!(f, "{} [{}/{:?}] {}", entry.ts, entry.thread, entry.level, entry.msg);
                }
                let _ = app.emit("game://log", &entry);
            }
        })
    };

    pump(Box::new(BufReader::new(stdout)), false);
    pump(Box::new(BufReader::new(stderr)), true);

    // Reaper: report the exit code so the UI can distinguish a clean quit from
    // a crash, and trigger crash recovery when it is non-zero.
    {
        let app = app.clone();
        let session = session.clone();
        thread::spawn(move || {
            let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
            session.running.store(false, Ordering::Relaxed);
            let _ = app.emit("game://exit", code);
            if code != 0 {
                let _ = app.emit("game://crash", code);
            }
        });
    }

    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_log4j() {
        let (t, l, m) = parse("[21:14:02] [Render thread/WARN]: Mipmap clamped", false);
        assert_eq!(t, "Render thread");
        assert_eq!(l, Level::Warn);
        assert_eq!(m, "Mipmap clamped");
    }

    #[test]
    fn unparsed_stderr_becomes_error() {
        let (t, l, m) = parse("\tat net.minecraft.Foo.bar(Foo.java:12)", true);
        assert_eq!(t, "stderr");
        assert_eq!(l, Level::Error);
        assert!(m.contains("Foo.java"));
    }

    #[test]
    fn thread_names_with_slashes() {
        let (t, l, _) = parse("[10:00:00] [main/worker/INFO]: hi", false);
        assert_eq!(t, "main/worker");
        assert_eq!(l, Level::Info);
    }
}
