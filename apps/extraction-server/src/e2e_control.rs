use std::{
    io::{self, BufRead, Write},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tokio::sync::oneshot;

use crate::ports::Clock;

const ADVANCE_PREFIX: &str = "advance_ms ";
const MAX_ADVANCE_MS: u64 = 15 * 60 * 1_000;
const MAX_COMMAND_BYTES: usize = 64;

#[derive(Debug)]
struct ClockState {
    elapsed_ms: u64,
    utc_now: SystemTime,
}

/// 仅由 `e2e-control` feature 构建；单一偏移同时驱动单调时间和 UTC。
#[derive(Debug)]
pub(crate) struct E2eControlClock {
    state: Mutex<ClockState>,
}

impl Default for E2eControlClock {
    fn default() -> Self {
        Self::new(SystemTime::now())
    }
}

impl E2eControlClock {
    fn new(utc_now: SystemTime) -> Self {
        Self {
            state: Mutex::new(ClockState {
                elapsed_ms: 0,
                utc_now,
            }),
        }
    }

    fn try_advance(&self, advance_ms: u64) -> Result<ClockSnapshot, ControlCode> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let elapsed_ms = state
            .elapsed_ms
            .checked_add(advance_ms)
            .ok_or(ControlCode::ClockOverflow)?;
        let utc_now = state
            .utc_now
            .checked_add(Duration::from_millis(advance_ms))
            .ok_or(ControlCode::ClockOverflow)?;
        let utc_unix_ms = utc_now
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_millis()).ok())
            .ok_or(ControlCode::ClockOverflow)?;
        state.elapsed_ms = elapsed_ms;
        state.utc_now = utc_now;
        Ok(ClockSnapshot {
            monotonic_ms: elapsed_ms,
            utc_unix_ms,
        })
    }
}

impl Clock for E2eControlClock {
    fn monotonic_now(&self) -> Duration {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        Duration::from_millis(state.elapsed_ms)
    }

    fn utc_now(&self) -> SystemTime {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.utc_now
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ControlCode {
    InvalidCommand,
    CommandTooLong,
    AdvanceTooLarge,
    ClockOverflow,
    StdinClosed,
    StdinReadFailed,
}

impl ControlCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidCommand => "INVALID_COMMAND",
            Self::CommandTooLong => "COMMAND_TOO_LONG",
            Self::AdvanceTooLarge => "ADVANCE_TOO_LARGE",
            Self::ClockOverflow => "CLOCK_OVERFLOW",
            Self::StdinClosed => "STDIN_CLOSED",
            Self::StdinReadFailed => "STDIN_READ_FAILED",
        }
    }

    const fn error_kind(self) -> io::ErrorKind {
        match self {
            Self::StdinClosed => io::ErrorKind::UnexpectedEof,
            Self::StdinReadFailed => io::ErrorKind::Other,
            Self::ClockOverflow => io::ErrorKind::InvalidData,
            Self::InvalidCommand | Self::CommandTooLong | Self::AdvanceTooLarge => {
                io::ErrorKind::InvalidInput
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct ClockSnapshot {
    monotonic_ms: u64,
    utc_unix_ms: u64,
}

#[derive(Debug, Serialize)]
struct SuccessAck {
    event: &'static str,
    status: &'static str,
    advance_ms: u64,
    #[serde(flatten)]
    clock: ClockSnapshot,
}

#[derive(Debug, Serialize)]
struct ErrorAck {
    event: &'static str,
    status: &'static str,
    code: &'static str,
}

pub(crate) fn start_stdin_controller(
    clock: Arc<E2eControlClock>,
) -> io::Result<oneshot::Receiver<io::Error>> {
    let (failure_sender, failure_receiver) = oneshot::channel();
    thread::Builder::new()
        .name("e2e-clock-control".to_owned())
        .spawn(move || {
            let stdin = io::stdin();
            let stdout = io::stdout();
            let error = run_control_loop(stdin.lock(), stdout, clock.as_ref())
                .expect_err("stdin control loop only exits by failing closed");
            let _ = failure_sender.send(error);
        })?;
    Ok(failure_receiver)
}

fn run_control_loop(
    mut input: impl BufRead,
    mut output: impl Write,
    clock: &E2eControlClock,
) -> io::Result<()> {
    loop {
        let line = match read_control_line(&mut input) {
            Ok(Some(line)) => line,
            Ok(None) => return reject(&mut output, ControlCode::StdinClosed),
            Err(code) => return reject(&mut output, code),
        };
        let advance_ms = match parse_advance_ms(&line) {
            Ok(advance_ms) => advance_ms,
            Err(code) => return reject(&mut output, code),
        };
        let snapshot = match clock.try_advance(advance_ms) {
            Ok(snapshot) => snapshot,
            Err(code) => return reject(&mut output, code),
        };
        write_json_line(
            &mut output,
            &SuccessAck {
                event: "e2e_clock_ack",
                status: "ok",
                advance_ms,
                clock: snapshot,
            },
        )?;
    }
}

fn parse_advance_ms(line: &str) -> Result<u64, ControlCode> {
    let value = line
        .strip_prefix(ADVANCE_PREFIX)
        .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or(ControlCode::InvalidCommand)?;
    let advance_ms = value
        .parse::<u64>()
        .map_err(|_| ControlCode::InvalidCommand)?;
    if advance_ms > MAX_ADVANCE_MS {
        return Err(ControlCode::AdvanceTooLarge);
    }
    Ok(advance_ms)
}

fn read_control_line(input: &mut impl BufRead) -> Result<Option<String>, ControlCode> {
    let mut line = Vec::with_capacity(ADVANCE_PREFIX.len() + 20);
    loop {
        let available = input.fill_buf().map_err(|_| ControlCode::StdinReadFailed)?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(ControlCode::InvalidCommand)
            };
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            extend_bounded(&mut line, &available[..newline])?;
            input.consume(newline + 1);
            break;
        }
        let consumed = available.len();
        extend_bounded(&mut line, available)?;
        input.consume(consumed);
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|_| ControlCode::InvalidCommand)
}

fn extend_bounded(line: &mut Vec<u8>, bytes: &[u8]) -> Result<(), ControlCode> {
    if line
        .len()
        .checked_add(bytes.len())
        .is_none_or(|length| length > MAX_COMMAND_BYTES)
    {
        return Err(ControlCode::CommandTooLong);
    }
    line.extend_from_slice(bytes);
    Ok(())
}

fn reject(output: &mut impl Write, code: ControlCode) -> io::Result<()> {
    write_json_line(
        output,
        &ErrorAck {
            event: "e2e_clock_ack",
            status: "error",
            code: code.as_str(),
        },
    )?;
    Err(io::Error::new(code.error_kind(), code.as_str()))
}

fn write_json_line(output: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    let mut line = serde_json::to_vec(value).map_err(io::Error::other)?;
    line.push(b'\n');
    output.write_all(&line)?;
    output.flush()
}

#[cfg(test)]
#[path = "e2e_control_tests.rs"]
mod tests;
