#[allow(dead_code)]
#[path = "../thumbnail.rs"]
mod thumbnail;

use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use thumbnail::helper_wire::{decode_thumbnail_helper_request, encode_thumbnail_helper_report};
use thumbnail::{capture_toplevel_thumbnails_with_timeout, ToplevelThumbnailReport};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nuraloumi-thumbnail-helper: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let mut request_bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut request_bytes)
        .map_err(|error| format!("read request failed: {error}"))?;
    let requests = decode_thumbnail_helper_request(&request_bytes)?;
    let report =
        match capture_toplevel_thumbnails_with_timeout(&requests, Duration::from_millis(350)) {
            Ok(report) => report,
            Err(error) => ToplevelThumbnailReport {
                issues: vec![error],
                ..ToplevelThumbnailReport::default()
            },
        };
    let response = encode_thumbnail_helper_report(&report)?;
    std::io::stdout()
        .write_all(&response)
        .map_err(|error| format!("write response failed: {error}"))?;
    Ok(())
}
