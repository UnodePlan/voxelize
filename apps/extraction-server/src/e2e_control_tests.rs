use std::{io::Cursor, time::UNIX_EPOCH};

use serde_json::Value;

use super::*;

#[test]
fn one_offset_advances_monotonic_and_utc_together() {
    let base = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let clock = E2eControlClock::new(base);

    let snapshot = clock.try_advance(480_000).unwrap();

    assert_eq!(snapshot.monotonic_ms, 480_000);
    assert_eq!(clock.monotonic_now(), Duration::from_secs(480));
    assert_eq!(clock.utc_now(), base + Duration::from_secs(480));
    assert_eq!(snapshot.utc_unix_ms, 1_800_000_480_000);
}

#[test]
fn strict_commands_emit_machine_readable_ack_and_never_echo_input() {
    let clock = E2eControlClock::new(UNIX_EPOCH + Duration::from_secs(1_800_000_000));
    let input = Cursor::new(b"advance_ms 480000\r\nadvance_ms 8000\nsecret token\n");
    let mut output = Vec::new();

    let error = run_control_loop(input, &mut output, &clock).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    let text = String::from_utf8(output).unwrap();
    assert!(!text.contains("secret"));
    let records = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["status"], "ok");
    assert_eq!(records[0]["advance_ms"], 480_000);
    assert_eq!(records[1]["monotonic_ms"], 488_000);
    assert_eq!(records[2]["status"], "error");
    assert_eq!(records[2]["code"], "INVALID_COMMAND");
}

#[test]
fn per_command_limit_is_enforced_without_advancing_clock() {
    let clock = E2eControlClock::new(UNIX_EPOCH);
    let command = format!("advance_ms {}\n", MAX_ADVANCE_MS + 1);
    let mut output = Vec::new();

    let error = run_control_loop(Cursor::new(command), &mut output, &clock).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(clock.monotonic_now(), Duration::ZERO);
    let ack: Value = serde_json::from_slice(output.trim_ascii()).unwrap();
    assert_eq!(ack["code"], "ADVANCE_TOO_LARGE");
}

#[test]
fn parser_accepts_only_the_exact_unsigned_decimal_shape() {
    assert_eq!(parse_advance_ms("advance_ms 0"), Ok(0));
    assert_eq!(
        parse_advance_ms(&format!("advance_ms {MAX_ADVANCE_MS}")),
        Ok(MAX_ADVANCE_MS)
    );
    for command in [
        "advance_ms",
        "advance_ms  1",
        "advance_ms +1",
        "advance_ms -1",
        "advance_ms\t1",
        "advance_ms 1 ",
        "advance_ms 1ms",
        "advance_ms 18446744073709551616",
    ] {
        assert_eq!(
            parse_advance_ms(command),
            Err(ControlCode::InvalidCommand),
            "command unexpectedly accepted"
        );
    }
}

#[test]
fn elapsed_overflow_fails_without_partial_utc_advance() {
    let base = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let clock = E2eControlClock::new(base);
    {
        let mut state = clock.state.lock().unwrap();
        state.elapsed_ms = u64::MAX - 1;
    }

    assert!(matches!(
        clock.try_advance(2),
        Err(ControlCode::ClockOverflow)
    ));
    assert_eq!(clock.utc_now(), base);
    assert_eq!(clock.monotonic_now(), Duration::from_millis(u64::MAX - 1));
}

#[test]
fn unterminated_and_oversized_lines_fail_closed() {
    let clock = E2eControlClock::new(UNIX_EPOCH);
    let mut unterminated_output = Vec::new();
    let unterminated = run_control_loop(
        Cursor::new(b"advance_ms 1"),
        &mut unterminated_output,
        &clock,
    )
    .unwrap_err();
    assert_eq!(unterminated.kind(), io::ErrorKind::InvalidInput);

    let mut oversized_output = Vec::new();
    let oversized = format!("{}\n", "x".repeat(MAX_COMMAND_BYTES + 1));
    let oversized =
        run_control_loop(Cursor::new(oversized), &mut oversized_output, &clock).unwrap_err();
    assert_eq!(oversized.kind(), io::ErrorKind::InvalidInput);
    let ack: Value = serde_json::from_slice(oversized_output.trim_ascii()).unwrap();
    assert_eq!(ack["code"], "COMMAND_TOO_LONG");
}

#[test]
fn clean_stdin_eof_is_a_machine_readable_shutdown_error() {
    let clock = E2eControlClock::new(UNIX_EPOCH);
    let mut output = Vec::new();

    let error = run_control_loop(Cursor::new([]), &mut output, &clock).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    let ack: Value = serde_json::from_slice(output.trim_ascii()).unwrap();
    assert_eq!(ack["code"], "STDIN_CLOSED");
}
