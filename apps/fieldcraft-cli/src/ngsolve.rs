//! Headless entry point for the same NGSolve analysis runner as the desktop.

use std::io::Write;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use fieldcraft_engine::ngsolve::{Analysis, SOLVER_TIMEOUT};

use crate::Error;

pub(crate) fn run(args: &[String], out: &mut impl Write) -> Result<(), Error> {
    let [action, source, output, flag, interpreter] = args else {
        return Err("usage: fieldcraft-cli ngsolve run <analysis.analysis.json> <output> --python PATH".into());
    };
    if action != "run" || flag != "--python" {
        return Err("usage: fieldcraft-cli ngsolve run <analysis.analysis.json> <output> --python PATH".into());
    }
    let python = std::path::absolute(interpreter).map_err(|error| error.to_string())?;
    let prepared =
        Analysis::read(Path::new(source)).and_then(|analysis| analysis.prepare(Path::new(output), None)).map_err(|error| error.to_string())?;
    let mut command = prepared.command(&python).map_err(|error| error.to_string())?;
    let mut child = command.stdin(Stdio::null()).spawn().map_err(|error| error.to_string())?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() < SOLVER_TIMEOUT => std::thread::sleep(Duration::from_millis(50)),
            outcome => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(outcome.err().map_or_else(|| "NGSolve exceeded the one-hour analysis time limit".to_string(), |e| e.to_string()).into());
            }
        }
    };
    if !status.success() {
        return Err(format!("NGSolve exited with {status}").into());
    }
    let path = prepared.verify(&python).and_then(|run| run.publish()).map_err(|error| error.to_string())?;
    writeln!(out, "{}", serde_json::json!({"results":path}))?;
    Ok(())
}
