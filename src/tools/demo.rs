use super::*;
pub struct Demo;
impl Tool for Demo {
    fn id(&self) -> &'static str {
        "demo"
    }
    fn name(&self) -> &'static str {
        "本地演示 / 自检"
    }
    fn description(&self) -> &'static str {
        "随项目构建的独立 CLI：验证参数、stdout、stderr、退出码和文件输出"
    }
    fn executable(&self) -> &'static str {
        "toolkit-demo"
    }
    fn fields(&self) -> Vec<Field> {
        vec![
            Field {
                key: "message",
                label: "消息",
                kind: FieldKind::Text,
                default: "你好，Apple Toolkit",
                required: true,
            },
            choice("mode", "运行模式", &["success", "error", "slow"]),
            Field {
                key: "input",
                label: "输入文件（可选）",
                kind: FieldKind::File,
                default: "",
                required: false,
            },
            Field {
                key: "output",
                label: "输出文件（可选）",
                kind: FieldKind::SaveFile,
                default: "",
                required: false,
            },
        ]
    }
    fn args(&self, v: &Values) -> Result<Vec<OsString>, String> {
        validate(self, v)?;
        if !value(v, "input").is_empty() && !std::path::Path::new(value(v, "input")).is_file() {
            return Err("输入文件不存在".into());
        }
        let mut args = vec![
            "--message".into(),
            value(v, "message").into(),
            "--mode".into(),
            value(v, "mode").into(),
        ];
        for key in ["input", "output"] {
            if !value(v, key).is_empty() {
                args.extend([format!("--{key}").into(), value(v, key).into()]);
            }
        }
        Ok(args)
    }
}
