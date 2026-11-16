use super::*;
pub struct DeviceId;
impl Tool for DeviceId {
    fn id(&self) -> &'static str {
        "device_id"
    }
    fn name(&self) -> &'static str {
        "USB 设备列表"
    }
    fn description(&self) -> &'static str {
        "idevice_id：列出通过 USB 连接的设备标识"
    }
    fn executable(&self) -> &'static str {
        "idevice_id"
    }
    fn fields(&self) -> Vec<Field> {
        vec![]
    }
    fn args(&self, _: &Values) -> Result<Vec<OsString>, String> {
        Ok(vec!["-l".into()])
    }
}
