mod native;
use crate::{mdm::Device, models};
use plist::Value;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Serialize, Deserialize)]
pub struct Row {
    pub item: String,
    pub current: String,
    pub result: String,
    pub source: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Report {
    pub device: Device,
    pub read_at_unix: u64,
    pub rows: Vec<Row>,
    pub notes: Vec<String>,
}
impl Report {
    pub fn read_at_label(&self) -> String {
        time::OffsetDateTime::from_unix_timestamp(self.read_at_unix as i64)
            .ok()
            .map(|t| {
                t.to_offset(time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC))
            })
            .and_then(|t| {
                t.format(&time::format_description::well_known::Rfc3339)
                    .ok()
            })
            .unwrap_or_else(|| "读取时间不可用".into())
            .replace('T', " ")
    }
    pub fn base(device: Device) -> Self {
        let mut rows = vec![];
        for (item, value, source) in [
            (
                "设备型号",
                format!("{} ({})", models::name(&device.product), device.product),
                "lockdownd/ProductType",
            ),
            (
                "销售型号",
                fact(&device, "ModelNumber"),
                "lockdownd/ModelNumber",
            ),
            (
                "销售地区",
                fact(&device, "RegionInfo"),
                "lockdownd/RegionInfo",
            ),
            (
                "监管型号",
                fact(&device, "RegulatoryModelNumber"),
                "lockdownd/RegulatoryModelNumber",
            ),
            (
                "硬盘容量",
                fact(&device, "CapacityLabel"),
                "lockdownd/com.apple.disk_usage/TotalDiskCapacity（换算）",
            ),
            (
                "Wi-Fi地址",
                fact(&device, "WiFiAddress"),
                "lockdownd/WiFiAddress",
            ),
            (
                "蓝牙地址",
                fact(&device, "BluetoothAddress"),
                "lockdownd/BluetoothAddress",
            ),
            (
                "蜂窝/以太网地址",
                fact(&device, "EthernetAddress"),
                "lockdownd/EthernetAddress",
            ),
            (
                "整机序列号",
                device.serial.clone(),
                "lockdownd/SerialNumber",
            ),
            (
                "主板序列号",
                fact(&device, "MLBSerialNumber"),
                "lockdownd/MLBSerialNumber",
            ),
            (
                "系统版本",
                device.version.clone(),
                "lockdownd/ProductVersion",
            ),
            (
                "系统构建版本",
                device.build.clone(),
                "lockdownd/BuildVersion",
            ),
            (
                "IMEI",
                device.imei.clone(),
                "lockdownd/InternationalMobileEquipmentIdentity",
            ),
            (
                "当前电量",
                fact(&device, "BatteryLevelLabel"),
                "lockdownd/com.apple.mobile.battery/BatteryCurrentCapacity",
            ),
        ] {
            rows.push(row(item, value, source));
        }
        for item in [
            "电池序列号",
            "电池循环次数",
            "电池健康度",
            "电池生产日期",
            "前置摄像头",
            "后置摄像头",
            "超广角模组",
            "长焦模组",
            "点阵模组",
            "红外模组",
            "前盖板码",
            "后盖板码",
            "屏幕序列号",
            "触摸序列号",
            "振动马达",
        ] {
            rows.push(row(item, String::new(), "设备诊断接口"));
        }
        Self {
            device,
            read_at_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            rows,
            notes: vec![
                "数据来自本次连接设备的实际响应。".into(),
                "未获取的项目可能受系统版本、权限或设备服务限制，不据此判断故障。".into(),
            ],
        }
    }
    pub fn inspect(&mut self, root: &Path) {
        let (trees, notes) = native::read(root, &self.device.udid);
        self.notes.extend(notes);
        for (item, aliases, battery_only) in [
            ("监管型号", &["RegulatoryModelNumber"][..], false),
            (
                "电池序列号",
                &[
                    "BatterySerialNumber",
                    "battery-serial-number",
                    "SerialNumber",
                    "Serial",
                ][..],
                true,
            ),
            (
                "电池循环次数",
                &["CycleCount", "BatteryCycleCount"][..],
                true,
            ),
            (
                "电池健康度",
                &["MaximumCapacityPercent", "MaximumCapacityPercentage"][..],
                true,
            ),
            (
                "电池生产日期",
                &["ManufactureDate", "ManufacturingDate"][..],
                true,
            ),
            (
                "前置摄像头",
                &[
                    "FrontCameraSerialNumber",
                    "FrontFacingCameraSerialNumber",
                    "front-camera-serial-number",
                ][..],
                false,
            ),
            (
                "后置摄像头",
                &[
                    "RearCameraSerialNumber",
                    "RearFacingCameraSerialNumber",
                    "back-camera-serial-number",
                ][..],
                false,
            ),
            ("超广角模组", &["UltraWideCameraSerialNumber"][..], false),
            (
                "长焦模组",
                &["TelephotoCameraSerialNumber", "TeleCameraSerialNumber"][..],
                false,
            ),
            ("点阵模组", &["DotProjectorSerialNumber"][..], false),
            (
                "红外模组",
                &["InfraredCameraSerialNumber", "IRCameraSerialNumber"][..],
                false,
            ),
            (
                "前盖板码",
                &["CoverGlassSerialNumber", "FrontCoverSerialNumber"][..],
                false,
            ),
            (
                "后盖板码",
                &["BackCoverSerialNumber", "RearCoverSerialNumber"][..],
                false,
            ),
            (
                "屏幕序列号",
                &[
                    "DisplaySerialNumber",
                    "LCDSerialNumber",
                    "display-serial-number",
                ][..],
                false,
            ),
            (
                "触摸序列号",
                &["TouchSerialNumber", "DigitizerSerialNumber"][..],
                false,
            ),
            (
                "振动马达",
                &["TapticEngineSerialNumber", "HapticEngineSerialNumber"][..],
                false,
            ),
        ] {
            let mut values = BTreeMap::<String, String>::new();
            for tree in &trees {
                if battery_only
                    && !tree.source.contains("gestalt")
                    && !tree.source.contains("GasGauge")
                    && !tree.source.contains("AppleSmartBattery")
                {
                    continue;
                }
                let mut found = vec![];
                let scoped: Vec<&str> = if battery_only && tree.source.contains("gestalt") {
                    aliases
                        .iter()
                        .copied()
                        .filter(|key| key.starts_with("Battery"))
                        .collect()
                } else {
                    aliases.to_vec()
                };
                find(&tree.value, &scoped, "", 0, &mut found);
                for (value, path) in found {
                    values
                        .entry(value)
                        .or_insert_with(|| format!("{}{}", tree.source, path));
                }
            }
            if let Some(record) = self.rows.iter_mut().find(|r| r.item == item) {
                if values.len() == 1 {
                    let (mut value, source) = values.into_iter().next().unwrap();
                    if item == "电池健康度" {
                        if let Some(n) = value
                            .parse::<f64>()
                            .ok()
                            .filter(|n| *n >= 0.0 && *n <= 100.0)
                        {
                            value = format!("{n}%（设备报告）");
                        }
                    }
                    record.current = value;
                    record.source = source;
                    record.result = "已读取".into();
                    if item == "电池生产日期" {
                        record.current = format!("原始编码：{}（尚未解码）", record.current);
                        record.result = "已读取编码，尚未解码".into();
                    }
                } else if values.len() > 1 {
                    record.current = values.keys().cloned().collect::<Vec<_>>().join(" / ");
                    record.result = "读取到多个值，请查看数据来源".into();
                }
            }
        }
    }
    pub fn html(&self) -> String {
        let mut s=format!("<!doctype html><html lang=zh-CN><meta charset=utf-8><title>Unbound设备信息报告</title><style>body{{font:15px system-ui;margin:32px;color:#233247}}table{{border-collapse:collapse;width:100%}}td,th{{padding:11px;text-align:left;border-bottom:1px solid #dde5ee;overflow-wrap:anywhere}}th{{background:#eff6ff}}</style><h1>{} · 设备信息报告</h1><p>序列号：{}　系统：{}</p><p>读取时间：{}</p><table><tr><th>检测项目</th><th>实际读取值</th><th>读取说明</th></tr>",escape(models::name(&self.device.product)),escape(&self.device.serial),escape(&self.device.version),escape(&self.read_at_label()));
        for r in &self.rows {
            s.push_str(&format!(
                "<tr title=\"{}\"><td>{}</td><td>{}</td><td>{}</td></tr>",
                escape(&r.source),
                escape(&r.item),
                escape(&r.current),
                escape(&r.result)
            ));
        }
        s.push_str("</table>");
        for note in &self.notes {
            s.push_str(&format!("<p>{}</p>", escape(note)));
        }
        s.push_str("</html>");
        s
    }
}
fn row(item: &str, current: String, source: &str) -> Row {
    let available = !current.is_empty();
    Row {
        item: item.into(),
        current: if available {
            current
        } else {
            "未获取".into()
        },
        result: if available { "已读取" } else { "未获取" }.into(),
        source: source.into(),
    }
}
fn fact(device: &Device, key: &str) -> String {
    device.facts.get(key).cloned().unwrap_or_default()
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Integer(i) => Some(i.to_string()),
        Value::Boolean(b) => Some(b.to_string()),
        Value::Real(n) => Some(n.to_string()),
        Value::Data(b) => {
            let b = b.strip_suffix(&[0]).unwrap_or(b);
            std::str::from_utf8(b)
                .ok()
                .filter(|s| s.chars().all(|c| !c.is_control()))
                .map(str::to_owned)
        }
        _ => None,
    }
}
fn find(value: &Value, keys: &[&str], path: &str, depth: usize, out: &mut Vec<(String, String)>) {
    if depth > 64 || out.len() > 128 {
        return;
    }
    match value {
        Value::Dictionary(d) => {
            for (key, v) in d {
                let next = format!("{path}/{key}");
                if keys.iter().any(|k| key.eq_ignore_ascii_case(k)) {
                    if let Some(t) = text(v).filter(|s| !s.is_empty()) {
                        out.push((t, next.clone()));
                    }
                }
                find(v, keys, &next, depth + 1, out);
            }
        }
        Value::Array(a) => {
            for (n, v) in a.iter().enumerate() {
                find(v, keys, &format!("{path}/{n}"), depth + 1, out);
            }
        }
        _ => (),
    }
}
pub fn capacity_label(bytes: u64) -> String {
    let gb = bytes as f64 / 1_000_000_000.0;
    for nominal in [8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0, 2048.0] {
        if (gb - nominal).abs() / nominal < 0.10 {
            return format!("{nominal:.0} GB（由系统容量换算）");
        }
    }
    format!("{gb:.1} GB（系统报告）")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_contains_only_current_readings() {
        let d = Device {
            udid: "a".repeat(40),
            serial: "TEST".into(),
            product: "iPhone17,1".into(),
            version: "18.0".into(),
            build: "22A".into(),
            name: "<script>".into(),
            imei: String::new(),
            activation: "Activated".into(),
            facts: BTreeMap::new(),
        };
        let r = Report::base(d);
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("original"));
        assert!(r.rows.iter().any(|r| r.result == "未获取"));
        assert!(!r.rows.iter().any(|r| r.result == "正常"));
        assert!(r.html().contains("实际读取值"));
        assert!(!r.html().contains("原值"));
    }
    #[test]
    fn conflicting_components_are_not_equal_baselines() {
        let p = Value::Array(vec![
            Value::Dictionary(plist::Dictionary::from_iter([(
                String::from("CycleCount"),
                Value::Integer(12.into()),
            )])),
            Value::Dictionary(plist::Dictionary::from_iter([(
                String::from("CycleCount"),
                Value::Integer(48.into()),
            )])),
        ]);
        let mut values = vec![];
        find(&p, &["CycleCount"], "", 0, &mut values);
        assert_eq!(values.len(), 2);
    }
    #[test]
    fn html_escapes_device_values() {
        assert_eq!(escape("<script>\"x\"&"), "&lt;script&gt;&quot;x&quot;&amp;");
    }
}
