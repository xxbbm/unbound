//! 所有适配器共享表单 schema 和 argv 接口。禁止拼接 shell 命令字符串。
mod backup;
mod demo;
mod device_id;
mod device_info;

mod pair;

use std::{collections::BTreeMap, ffi::OsString};

pub type Values = BTreeMap<String, String>;

#[derive(Clone)]
pub enum FieldKind {
    Text,
    Choice(&'static [&'static str]),
    Bool,
    File,
    Directory,
    SaveFile,
}

#[derive(Clone)]
pub struct Field {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    pub default: &'static str,
    pub required: bool,
}

pub trait Tool: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn executable(&self) -> &'static str;
    fn fields(&self) -> Vec<Field>;
    fn args(&self, values: &Values) -> Result<Vec<OsString>, String>;
}

pub fn registry() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(device_id::DeviceId),
        Box::new(device_info::DeviceInfo),
        Box::new(pair::Pair),
        Box::new(backup::Backup),
        Box::new(demo::Demo),
    ]
}

pub fn text(key: &'static str, label: &'static str) -> Field {
    Field {
        key,
        label,
        kind: FieldKind::Text,
        default: "",
        required: false,
    }
}
pub fn choice(key: &'static str, label: &'static str, options: &'static [&'static str]) -> Field {
    Field {
        key,
        label,
        kind: FieldKind::Choice(options),
        default: options[0],
        required: true,
    }
}
pub fn value<'a>(v: &'a Values, key: &str) -> &'a str {
    v.get(key).map(String::as_str).unwrap_or("")
}
pub fn validate(tool: &dyn Tool, v: &Values) -> Result<(), String> {
    for f in tool.fields() {
        let s = value(v, f.key);
        if f.required && s.trim().is_empty() {
            return Err(format!("请填写{}", f.label));
        }
        if s.contains('\0') {
            return Err(format!("{}包含非法 NUL 字符", f.label));
        }
        if let FieldKind::Choice(options) = f.kind {
            if !options.contains(&s) {
                return Err(format!("{}选项无效", f.label));
            }
        }
    }
    Ok(())
}
pub fn udid(v: &Values, args: &mut Vec<OsString>) {
    let s = value(v, "udid");
    if !s.trim().is_empty() {
        args.extend(["-u".into(), s.into()]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn info_options_are_separate_arguments() {
        let v = Values::from([
            ("udid".into(), "device with spaces".into()),
            ("key".into(), "ProductVersion".into()),
        ]);
        let args = device_info::DeviceInfo.args(&v).unwrap();
        assert_eq!(
            args,
            ["-u", "device with spaces", "-k", "ProductVersion"].map(OsString::from)
        );
    }
    #[test]
    fn backup_requires_output_and_preserves_path() {
        let mut v = Values::new();
        assert!(backup::Backup.args(&v).is_err());
        v.insert("directory".into(), "C:/测试 folder/backup".into());
        let args = backup::Backup.args(&v).unwrap();
        assert_eq!(args.last().unwrap(), "C:/测试 folder/backup");
    }
    #[test]
    fn rejects_unknown_command() {
        let v = Values::from([("action".into(), "injected".into())]);
        assert!(pair::Pair.args(&v).is_err());
    }
}
