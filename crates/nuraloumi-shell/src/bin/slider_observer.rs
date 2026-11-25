//! Opt-in, read-only sysfs binary-switch sampler. Never installed or autostarted.
use nuraloumi_providers::command::{
    CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner,
};
use nuraloumi_providers::slider::{SliderObserver, SliderState};
use std::{
    env,
    fs::File,
    io::Read,
    path::PathBuf,
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

fn sample(path: &PathBuf, open_level: u8) -> Option<SliderState> {
    let file = File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(64).read_to_end(&mut bytes).ok()?;
    let level = match std::str::from_utf8(&bytes).ok()?.trim() {
        "0" => 0,
        "1" => 1,
        _ => return None,
    };
    Some(if level == open_level {
        SliderState::Open
    } else {
        SliderState::Closed
    })
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let mut source = None;
    let mut open_level = None;
    let mut notify = false;
    let mut max_samples: Option<u64> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("nuraloumi-slider-observer --source /sys/... --open-level 0|1 [--notify] [--max-samples N]\nNo autodetection: physically prove source and polarity before --notify. Polls every 100 ms; 300 ms debounce; read-only, stdout dry-run by default.");
                return Ok(());
            }
            "--source" => {
                source = Some(PathBuf::from(args.next().ok_or("missing --source value")?))
            }
            "--open-level" => {
                open_level = Some(
                    args.next()
                        .ok_or("missing --open-level value")?
                        .parse::<u8>()
                        .map_err(|_| "invalid --open-level")?,
                )
            }
            "--notify" => notify = true,
            "--max-samples" => {
                max_samples = Some(
                    args.next()
                        .ok_or("missing --max-samples value")?
                        .parse::<u64>()
                        .map_err(|_| "invalid --max-samples")?,
                )
            }
            _ => return Err(format!("unknown argument {arg}")),
        }
    }
    let source = source.ok_or("--source is required")?;
    if !source.starts_with("/sys/") {
        return Err("source must be under /sys/".into());
    }
    let open_level = match open_level {
        Some(level @ (0 | 1)) => level,
        _ => return Err("--open-level must be 0 or 1".into()),
    };
    if max_samples == Some(0) {
        return Err("--max-samples must be positive".into());
    }
    let mut observer = SliderObserver::new(Duration::from_millis(300));
    let mut runner = SystemCommandRunner;
    let mut available = None;
    let mut count = 0u64;
    loop {
        let state = sample(&source, open_level);
        if available != Some(state.is_some()) {
            eprintln!(
                "slider sensor {}",
                if state.is_some() {
                    "available"
                } else {
                    "unavailable"
                }
            );
            available = Some(state.is_some());
        }
        if let Some(transition) = observer.observe(state, Instant::now()) {
            println!("{}", transition.summary());
            if notify {
                let status = runner.run(
                    &CommandSpec::new("nuraloumi-ctl").args([
                        "notify",
                        "--summary",
                        transition.summary(),
                        "--app",
                        "NuraLoumi",
                    ]),
                    CommandLimits::new(Duration::from_secs(2), 1024),
                );
                if !matches!(status, Ok(output) if output.status == 0) {
                    eprintln!("slider notification transport unavailable");
                }
            }
        }
        count += 1;
        if max_samples.is_some_and(|max| count >= max) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
