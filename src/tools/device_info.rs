use super::*;
pub struct DeviceInfo;
impl Tool for DeviceInfo {
    fn id(&self) -> &'static str {
        "device_info"
    }
    fn name(&self) -> &'static str {
        "设备信息"
    }
    fn description(&self) -> &'static str {
        "ideviceinfo：查询设备信息；UDID 留空时使用首台 USB 设备"
    }
    fn executable(&self) -> &'static str {
        "ideviceinfo"
    }
    fn fields(&self) -> Vec<Field> {
        vec![
            text("udid", "设备 UDID（可选）"),
            text("key", "查询键（可选，例如 ProductVersion）"),
            Field {
                key: "xml",
                label: "以 XML plist 输出",
                kind: FieldKind::Bool,
                default: "false",
                required: false,
            },
        ]
    }
    fn args(&self, v: &Values) -> Result<Vec<OsString>, String> {
        validate(self, v)?;
        let mut args = vec![];
        udid(v, &mut args);
        if !value(v, "key").trim().is_empty() {
            args.extend(["-k".into(), value(v, "key").into()]);
        }
        if value(v, "xml") == "true" {
            args.push("-x".into());
        }
        Ok(args)
    }
}
