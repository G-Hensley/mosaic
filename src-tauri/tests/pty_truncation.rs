#![cfg(windows)]

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

fn positioned_payload(len: usize) -> Vec<u8> {
    let mut payload = Vec::with_capacity(len);
    for offset in (0..len).step_by(16) {
        let token = format!("{offset:08}|abcdefg");
        payload.extend_from_slice(token.as_bytes());
    }
    payload.truncate(len);
    payload
}

fn read_line_through_conpty(payload: &[u8], delay_reader: bool) -> (bool, Vec<u8>) {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 32760,
            cols: 32760,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("open ConPTY");
    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let mut writer = pair.master.take_writer().expect("take writer");

    let mut command = CommandBuilder::new("cmd.exe");
    let script = if delay_reader {
        "ping -n 2 127.0.0.1 >nul & more"
    } else {
        "more"
    };
    command.args(["/d", "/q", "/c", script]);
    let mut child = pair.slave.spawn_command(command).expect("spawn reader");
    drop(pair.slave);

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut output = Vec::new();
        let result = reader.read_to_end(&mut output).map(|_| output);
        let _ = sender.send(result);
    });

    // ConPTY asks the terminal emulator for the cursor position during startup.
    // The real Mosaic terminal answers this; this headless test must do so too.
    thread::sleep(Duration::from_millis(50));
    writer.write_all(b"\x1b[1;1R").expect("answer cursor query");
    let mut input = payload.to_vec();
    input.extend_from_slice(b"\r\n");
    let write_ok = writer.write_all(&input).is_ok();
    let _ = writer.flush();
    thread::sleep(Duration::from_millis(if delay_reader { 1500 } else { 100 }));
    let _ = child.kill();
    let _ = child.wait();
    drop(writer);
    drop(pair.master);

    let output = receiver
        .recv_timeout(Duration::from_secs(15))
        .expect("reader timed out")
        .expect("read output");
    let offset = (0..payload.len())
        .step_by(16)
        .find(|offset| {
            let suffix = &payload[*offset..];
            output.windows(suffix.len()).any(|window| window == suffix)
        })
        .unwrap_or(payload.len());
    (write_ok, payload[offset..].to_vec())
}

#[test]
fn measure_conpty_single_write_delivery() {
    println!("requested received lost leading_loss write_all_ok suffix");
    for size in [512, 1024, 2048, 4096, 8192, 65536] {
        let payload = positioned_payload(size);
        let (write_ok, received) = read_line_through_conpty(&payload, false);
        let leading_loss = payload
            .windows(received.len())
            .position(|window| window == received)
            .unwrap_or(usize::MAX);
        let is_suffix = received.len() <= payload.len()
            && payload[payload.len() - received.len()..] == received;
        println!(
            "{size:9} {:8} {:4} {leading_loss:12} {write_ok:12} {is_suffix}",
            received.len(),
            size.saturating_sub(received.len())
        );
        assert!(write_ok, "write_all failed for {size} bytes");
        assert!(
            is_suffix,
            "received bytes were not a payload suffix at {size}"
        );
    }
}

#[test]
fn measure_conpty_write_before_child_drains() {
    let payload = positioned_payload(65536);
    let (write_ok, received) = read_line_through_conpty(&payload, true);
    println!(
        "delayed requested={} received={} lost={} write_all_ok={}",
        payload.len(),
        received.len(),
        payload.len().saturating_sub(received.len()),
        write_ok
    );
    assert!(write_ok);
    assert_eq!(received, payload);
}
