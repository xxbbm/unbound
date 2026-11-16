use apple_toolkit::runner::{self, Event, Request};
use std::{
    ffi::OsString,
    path::PathBuf,
    time::{Duration, Instant},
};

fn request(mode: &str) -> Request {
    Request {
        program: PathBuf::from(env!("CARGO_BIN_EXE_toolkit-demo")),
        args: ["--message", "中文 space ; & $(test)", "--mode", mode]
            .map(OsString::from)
            .into(),
        timeout: Duration::from_secs(10),
    }
}

#[test]
fn captures_both_streams_and_keeps_arguments_literal() {
    let run = runner::start(request("success"));
    let mut stdout = String::new();
    let mut stderr = String::new();
    loop {
        match run.events.recv_timeout(Duration::from_secs(15)).unwrap() {
            Event::Output { stderr: err, text } => {
                if err {
                    stderr.push_str(&text);
                } else {
                    stdout.push_str(&text);
                }
            }
            Event::Finished {
                code,
                cancelled,
                timed_out,
                ..
            } => {
                assert_eq!(code, Some(0));
                assert!(!cancelled && !timed_out);
                break;
            }
            Event::Error(e) => panic!("{e}"),
            _ => (),
        }
    }
    assert!(stdout.contains("中文 space ; & $(test)"));
    assert!(stderr.contains("标准错误流"));
}

#[test]
fn nonzero_exit_is_preserved() {
    let run = runner::start(request("error"));
    loop {
        match run.events.recv_timeout(Duration::from_secs(15)).unwrap() {
            Event::Finished { code, .. } => {
                assert_eq!(code, Some(7));
                break;
            }
            Event::Error(e) => panic!("{e}"),
            _ => (),
        }
    }
}

#[test]
fn cancellation_terminates_slow_process() {
    let run = runner::start(request("slow"));
    let start = Instant::now();
    loop {
        match run.events.recv_timeout(Duration::from_secs(15)).unwrap() {
            Event::Started(_) => run.cancel(),
            Event::Finished { cancelled, .. } => {
                assert!(cancelled);
                assert!(start.elapsed() < Duration::from_secs(15));
                break;
            }
            Event::Error(e) => panic!("{e}"),
            _ => (),
        }
    }
}

#[test]
fn timeout_and_missing_executable_are_reported() {
    let mut req = request("slow");
    req.timeout = Duration::from_millis(150);
    let run = runner::start(req);
    loop {
        match run.events.recv_timeout(Duration::from_secs(15)).unwrap() {
            Event::Finished { timed_out, .. } => {
                assert!(timed_out);
                break;
            }
            Event::Error(e) => panic!("{e}"),
            _ => (),
        }
    }
    let mut req = request("success");
    req.program = req.program.with_file_name("missing-tool-does-not-exist");
    let run = runner::start(req);
    assert!(matches!(
        run.events.recv_timeout(Duration::from_secs(15)).unwrap(),
        Event::Error(_)
    ));
}

#[test]
fn writes_output_file_with_unicode_and_spaces() {
    let root = std::env::temp_dir().join(format!("toolkit-{}-测试 space", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let output = root.join("结果 file.txt");
    let mut req = request("success");
    req.args
        .extend(["--output".into(), output.as_os_str().to_owned()]);
    let run = runner::start(req);
    loop {
        match run.events.recv_timeout(Duration::from_secs(15)).unwrap() {
            Event::Finished { code, .. } => {
                assert_eq!(code, Some(0));
                break;
            }
            Event::Error(e) => panic!("{e}"),
            _ => (),
        }
    }
    assert_eq!(
        std::fs::read_to_string(&output).unwrap(),
        "中文 space ; & $(test)"
    );
    std::fs::remove_file(output).unwrap();
    std::fs::remove_dir(root).unwrap();
}
