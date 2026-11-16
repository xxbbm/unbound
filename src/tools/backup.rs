use super::*;
pub struct Backup;
impl Tool for Backup {
    fn id(&self) -> &'static str {
        "backup"
    }
    fn name(&self) -> &'static str {
        "本地设备备份"
    }
    fn description(&self) -> &'static str {
        "idevicebackup2：向本地目录备份，或读取已有备份信息"
    }
    fn executable(&self) -> &'static str {
        "idevicebackup2"
    }
    fn fields(&self) -> Vec<Field> {
        vec![
            text("udid", "设备 UDID（可选）"),
            choice("action", "操作", &["backup", "info", "list"]),
            Field {
                key: "directory",
                label: "备份目录",
                kind: FieldKind::Directory,
                default: "",
                required: true,
            },
            Field {
                key: "full",
                label: "完整备份（仅 backup 使用）",
                kind: FieldKind::Bool,
                default: "false",
                required: false,
            },
        ]
    }
    fn args(&self, v: &Values) -> Result<Vec<OsString>, String> {
        // 单元测试/调用者也可以依赖默认操作。
        let mut v = v.clone();
        v.entry("action".into()).or_insert_with(|| "backup".into());
        validate(self, &v)?;
        let directory = std::path::Path::new(value(&v, "directory"));
        if directory.exists() && !directory.is_dir() {
            return Err("备份路径必须是目录".into());
        }
        if value(&v, "action") != "backup" && !directory.is_dir() {
            return Err("读取备份时目录必须已存在".into());
        }
        let mut args = vec![];
        udid(&v, &mut args);
        args.push(value(&v, "action").into());
        if value(&v, "action") == "backup" && value(&v, "full") == "true" {
            args.push("--full".into());
        }
        args.push(value(&v, "directory").into());
        Ok(args)
    }
}
