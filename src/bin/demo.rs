//! 无设备依赖、无网络代码的独立测试程序。
use std::{ffi::OsString, io::Write};
fn run() -> Result<i32, String> {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let (pairs, remainder) = args.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err("参数必须为 --key value 对".into());
    }
    let mut message = OsString::from("hello");
    let mut mode = OsString::from("success");
    let mut input = None;
    let mut output = None;
    for pair in pairs {
        match pair[0].to_str() {
            Some("--message") => message = pair[1].clone(),
            Some("--mode") => mode = pair[1].clone(),
            Some("--input") => input = Some(pair[1].clone()),
            Some("--output") => output = Some(pair[1].clone()),
            _ => return Err("未知参数".into()),
        }
    }
    if !["success", "error", "slow"].iter().any(|s| mode == *s) {
        return Err("未知模式".into());
    }
    println!("消息: {}", message.to_string_lossy());
    eprintln!("这是标准错误流演示，不代表运行失败。");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    if let Some(input) = input {
        let bytes = std::fs::read(input).map_err(|e| e.to_string())?;
        println!("输入文件: {} 字节", bytes.len());
    }
    if mode == "slow" {
        for n in 1..=30 {
            println!("进度 {n}/30");
            std::io::stdout().flush().map_err(|e| e.to_string())?;
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    if mode == "error" {
        eprintln!("模拟失败，退出码 7");
        return Ok(7);
    }
    if let Some(output) = output {
        std::fs::write(output, message.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        println!("输出文件已保存");
    }
    Ok(0)
}
fn main() {
    std::process::exit(match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            2
        }
    });
}
