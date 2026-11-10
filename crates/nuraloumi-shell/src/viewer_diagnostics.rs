//! File(1) fallback: exact argv, fixed output budget, hard wall-clock deadline.
//! Pipes are nonblocking; no reader thread can outlive the timeout.
use std::{
    io::{self, Read},
    os::{fd::AsRawFd, unix::process::CommandExt},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
const OUTPUT_CAP: usize = 4096;
const HELPER_TIMEOUT: Duration = Duration::from_secs(2);

pub fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\n' | '\t' => c,
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => '�',
            c if c.is_control() => '�',
            c => c,
        })
        .take(OUTPUT_CAP)
        .collect()
}

/// Keep the read side nonblocking even if a child leaves its pipe open in a
/// forked descendant. Killing the direct child is not sufficient by itself.
fn set_nonblocking(fd: i32) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Drain currently available bytes without waiting for EOF or reading beyond
/// the cap. Once full, remaining output is intentionally discarded.
fn read_available(pipe: &mut impl Read, output: &mut Vec<u8>, eof: &mut bool) -> io::Result<()> {
    if *eof {
        return Ok(());
    }
    loop {
        let remaining = OUTPUT_CAP.saturating_sub(output.len());
        if remaining == 0 {
            *eof = true;
            return Ok(());
        }
        let mut chunk = [0_u8; 512];
        let requested = remaining.min(chunk.len());
        match pipe.read(&mut chunk[..requested]) {
            Ok(0) => {
                *eof = true;
                return Ok(());
            }
            Ok(n) => output.extend_from_slice(&chunk[..n]),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
}

pub fn describe(path: &Path, explanation: &str) -> String {
    describe_with_program_timeout(path, explanation, "file", HELPER_TIMEOUT)
}

#[cfg(test)]
fn describe_with_program(path: &Path, explanation: &str, program: &str) -> String {
    describe_with_program_timeout(path, explanation, program, HELPER_TIMEOUT)
}

fn describe_with_program_timeout(
    path: &Path,
    explanation: &str,
    program: &str,
    timeout: Duration,
) -> String {
    // Explicit "--" keeps names starting with '-' safe. Never invoke a shell.
    // An isolated process group also lets us stop pipe-holding descendants.
    let mut child = match Command::new(program)
        .arg("--")
        .arg(path)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return format!(
                "{}\nfile(1) unavailable: {}",
                sanitize(explanation),
                sanitize(&error.to_string())
            );
        }
    };
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let init =
        set_nonblocking(stdout.as_raw_fd()).and_then(|()| set_nonblocking(stderr.as_raw_fd()));
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut status = None;
    let mut timed_out = false;
    let mut failure = None;
    let mut out_eof = false;
    let mut err_eof = false;
    if let Err(error) = init {
        failure = Some(format!("nonblocking helper pipes unavailable: {error}"));
    } else {
        let start = Instant::now();
        loop {
            if let Err(error) = read_available(&mut stdout, &mut out, &mut out_eof)
                .and_then(|()| read_available(&mut stderr, &mut err, &mut err_eof))
            {
                failure = Some(format!("reading file(1) output failed: {error}"));
                break;
            }
            if status.is_none() {
                match child.try_wait() {
                    Ok(Some(exit)) => status = Some(exit),
                    Ok(None) => {}
                    Err(error) => {
                        failure = Some(format!("waiting for file(1) failed: {error}"));
                        break;
                    }
                }
            }
            if status.is_some() && out_eof && err_eof {
                break;
            }
            let Some(remaining) = timeout.checked_sub(start.elapsed()) else {
                timed_out = true;
                break;
            };
            thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }
    if timed_out || failure.is_some() {
        // The child was started in its own PGID (its PID). This also kills
        // descendants that inherited stdout/stderr after the parent exits.
        if let Ok(pgid) = i32::try_from(child.id()) {
            unsafe { libc::kill(-pgid, libc::SIGKILL) };
        }
        let _ = child.kill();
    }
    if status.is_none() {
        // A killed process can still be stuck in uninterruptible kernel I/O.
        // Never let a blocking wait() invalidate the wall-clock deadline;
        // reap asynchronously if try_wait() cannot reap immediately.
        match child.try_wait() {
            Ok(Some(exit)) if !timed_out && failure.is_none() => status = Some(exit),
            Ok(Some(_)) => {}
            Ok(None) => {
                thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(error) => {
                failure.get_or_insert_with(|| format!("file(1) wait failed: {error}"));
            }
        }
    }
    let mut lines = vec![sanitize(explanation)];
    if timed_out {
        lines.push("file(1) exceeded its time limit".into());
    }
    if let Some(message) = failure {
        lines.push(message);
    }
    if let Some(code) = status {
        if !code.success() {
            lines.push(format!("file(1) exit status: {code}"));
        }
    }
    if !out.is_empty() {
        lines.push(sanitize(&String::from_utf8_lossy(&out)));
    }
    if !err.is_empty() {
        lines.push(format!(
            "file(1) stderr: {}",
            sanitize(&String::from_utf8_lossy(&err))
        ));
    }
    if out.is_empty() && err.is_empty() {
        lines.push("file(1) returned no diagnostic".into());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_ansi_and_bidirectional_controls() {
        assert_eq!(sanitize("abc\x1b[31m\u{202e}hi\r"), "abc�[31m�hi�");
    }
    #[test]
    fn safe_argv_handles_nonexistent_and_unsafe_filenames() {
        let path =
            std::env::temp_dir().join(format!("nura-absent-{}-\n\x1b[31m", std::process::id()));
        let text = describe(&path, "not renderable");
        assert!(text.contains("not renderable"));
        assert!(!text.contains('\x1b'));
        assert!(!text.contains('\r'));
    }
    #[test]
    fn missing_file_helper_displays_an_error() {
        let output = describe_with_program(
            Path::new("/absent-viewer-fixture"),
            "unsupported image",
            "/__nuraloumi_no_such_file_binary__",
        );
        assert!(output.contains("file(1) unavailable"));
        assert!(output.contains("unsupported image"));
    }
    #[test]
    fn stdout_is_clipped() {
        assert_eq!(sanitize(&"x".repeat(10000)).len(), OUTPUT_CAP);
    }
    #[test]
    fn pipe_holding_descendant_cannot_extend_helper_deadline() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let script = std::env::temp_dir().join(format!(
            "nuraloumi-viewer-helper-{}-{nonce}",
            std::process::id()
        ));
        // Invoke this freshly written fixture through a stable interpreter,
        // not as a newly-created executable. Some runner filesystems can
        // return ETXTBSY between write and exec, masking the pipe-timeout
        // behavior the test is meant to verify.
        // The direct helper exits immediately; its descendant keeps both
        // stdout/stderr pipes open after the interpreter exits.
        fs::write(&script, b"#!/bin/sh\nsleep 5 &\nexit 0\n").unwrap();
        let start = Instant::now();
        let text = describe_with_program_timeout(
            &script,
            "not renderable",
            "/bin/sh",
            Duration::from_millis(150),
        );
        fs::remove_file(&script).unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(text.contains("time limit"), "{text}");
    }
}
