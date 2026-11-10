//! Exact-argv file(1) fallback, bounded in wall time, output and resources.
use std::{
    io::Read,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
const OUTPUT_CAP: usize = 4096;

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

pub fn describe(path: &Path, explanation: &str) -> String {
    describe_with_program(path, explanation, "file")
}
fn describe_with_program(path: &Path, explanation: &str, program: &str) -> String {
    // Use -- so leading '-' in a filename is not parsed as an option.
    let mut child = match Command::new(program)
        .arg("--")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(err) => {
            return format!(
                "{}\nfile(1) unavailable: {}",
                sanitize(explanation),
                sanitize(&err.to_string())
            )
        }
    };
    fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe
                .by_ref()
                .take((OUTPUT_CAP + 1) as u64)
                .read_to_end(&mut bytes);
            bytes.truncate(OUTPUT_CAP);
            bytes
        })
    }
    let out = drain(child.stdout.take().expect("piped stdout"));
    let err = drain(child.stderr.take().expect("piped stderr"));
    let deadline = Instant::now() + Duration::from_secs(2);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let output = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    let mut lines = vec![sanitize(explanation)];
    match status {
        None => lines.push("file(1) timed out or wait failed".into()),
        Some(status) if !status.success() => lines.push(format!("file(1) exit status: {status}")),
        _ => {}
    }
    if !output.is_empty() {
        lines.push(sanitize(&String::from_utf8_lossy(&output)));
    }
    if !stderr.is_empty() {
        lines.push(format!(
            "file(1) stderr: {}",
            sanitize(&String::from_utf8_lossy(&stderr))
        ));
    }
    if output.is_empty() && stderr.is_empty() {
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
}
