use super::*;
pub struct Pair;
impl Tool for Pair {
    fn id(&self) -> &'static str {
        "pair"
    }
    fn name(&self) -> &'static str {
        "设备配对"
    }
    fn description(&self) -> &'static str {
        "idevicepair：验证、建立或移除本机与设备的配对记录"
    }
    fn executable(&self) -> &'static str {
        "idevicepair"
    }
    fn fields(&self) -> Vec<Field> {
        vec![
            text("udid", "设备 UDID（可选）"),
            choice("action", "操作", &["validate", "pair", "unpair"]),
        ]
    }
    fn args(&self, v: &Values) -> Result<Vec<OsString>, String> {
        validate(self, v)?;
        let mut args = vec![];
        udid(v, &mut args);
        args.push(value(v, "action").into());
        Ok(args)
    }
}
