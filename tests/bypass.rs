//! `bypass` 模块的端到端测试：密钥 -> 本地服务 -> 信封 -> 备份重签发。
//!
//! 这里不检查任何设备，也不需要原始工具目录，因此可以随 `cargo test` 稳定运行。
//! 需要真机的那一步（备份还原）不在覆盖范围内。

use apple_toolkit::bypass::{self, backend, crypto, payloads, rekey, secrets};
use std::collections::BTreeMap;

fn sample_secrets() -> secrets::Secrets {
    secrets::Secrets {
        version_key: [3u8; 32],
        version_iv: [4u8; 16],
        backup_secret: [5u8; 32],
        offset: 0x726dc,
    }
}

fn sample_device() -> apple_toolkit::mdm::Device {
    apple_toolkit::mdm::Device {
        udid: "00008110-000A1C2E3F40001E".into(),
        serial: "M69PCG7LJX".into(),
        product: "iPhone14,2".into(),
        version: "17.5.1".into(),
        build: "21F90".into(),
        name: "测试机".into(),
        imei: String::new(),
        activation: String::new(),
        facts: BTreeMap::new(),
    }
}

#[test]
fn ifm1_container_round_trip() {
    let container = {
        let mut v = Vec::new();
        v.extend_from_slice(secrets::MAGIC);
        v.extend((0..32u8).map(|i| i.wrapping_mul(7).wrapping_add(1)));
        v.extend((0..16u8).map(|i| i.wrapping_mul(13).wrapping_add(3)));
        v.extend((0..32u8).map(|i| i.wrapping_mul(11).wrapping_add(5)));
        v
    };
    assert_eq!(container.len(), secrets::CONTAINER_LEN);

    let parsed = secrets::Secrets::from_container(&container, 0x726dc).expect("应能解析");
    assert_eq!(parsed.offset, 0x726dc);
    assert!(parsed.summary().contains("IFM1"));
}

#[test]
fn route_is_decided_by_major_version() {
    use bypass::{route, Completion, Route};
    assert_eq!(route(16), Route::LegacyGaval);
    assert_eq!(route(27), Route::Mdm27);
    assert_eq!(route(28), Route::Unsupported);

    // 只有旧路线改写备份里的设备身份；现代路线保留原始字节
    assert!(Route::LegacyGaval.rewrites_device_identity());
    assert!(!Route::Mdm27.rewrites_device_identity());

    // 完成判定只看输出内容，不看 HTTP 状态
    assert_eq!(
        bypass::completion(Route::Mdm27, "…. IKEY MDM Successful. …"),
        Completion::Complete
    );
    assert_eq!(
        bypass::completion(Route::Mdm27, "HTTP/1.1 200 OK"),
        Completion::Incomplete
    );
    assert!(!Route::Mdm27.steps().is_empty());
}

#[test]
fn pipeline_has_two_restores_and_task_layout() {
    use bypass::pipeline::{self, StageKind, TaskLayout};
    use bypass::Route;

    for route in [Route::LegacyGaval, Route::Mdm27] {
        let rounds: Vec<u8> = pipeline::stages(route)
            .iter()
            .filter_map(|s| match s.kind {
                StageKind::Restore { round } => Some(round),
                _ => None,
            })
            .collect();
        assert_eq!(rounds, vec![1, 2], "两次恢复是扫描到的结构");
    }

    let layout = TaskLayout::new(std::env::temp_dir().join("atk-e2e-task"), "00008110-000A1C2E3F40001E");
    assert!(layout.ikey().ends_with("ikey"));
    assert!(layout.backup().starts_with(layout.ikey()));
    assert_eq!(layout.env().len(), 2);

    // 旧路线恢复器：空 argv，cwd 为 ikey
    let cmd = pipeline::legacy_restore(&layout, "MDM_SETUP.exe");
    assert!(cmd.args.is_empty());
    assert_eq!(cmd.cwd, layout.ikey());

    // 现代路线：参数没还原就不许猜
    assert!(pipeline::modern_restore(&layout, "restore.exe", Vec::new(), None).is_err());

    layout.create().expect("应能建目录");
    assert!(layout.ikey().is_dir());
    let _ = std::fs::remove_dir_all(layout.root());
}

#[test]
fn judge_downgrades_marker_with_errors() {
    use bypass::judge::{self, State};
    use bypass::Route;

    assert_eq!(
        judge::judge(Route::LegacyGaval, "IKEY MDM Successful.").state,
        State::Complete
    );
    assert_eq!(
        judge::judge(Route::LegacyGaval, "IKEY MDM Successful. Error: x").state,
        State::Doubtful
    );
    assert_eq!(
        judge::judge(Route::Mdm27, "First restore complete, waiting for device restart").state,
        State::Incomplete
    );
}

#[test]
fn activation_plan_matches_the_recovered_endpoints() {
    use bypass::activation;

    let steps = activation::plan();
    assert_eq!(steps.first().unwrap().label, "Starting activation...");
    assert_eq!(steps.last().unwrap().label, "Apple activation complete");
    assert!(!activation::is_done("Starting activation..."));
    assert!(activation::is_done("Apple activation complete"));

    let argv = activation::ideviceactivation_argv("00008110-000A1C2E3F40001E");
    assert_eq!(argv[0], "activate");
    assert_eq!(argv.last().unwrap(), activation::DEVICE_ACTIVATION);
}

// ---------------------------------------------------------------------------
// 执行器：本地计划 + 阶段推进
// ---------------------------------------------------------------------------

fn seed_backup(layout: &bypass::TaskLayout) -> std::io::Result<()> {
    std::fs::create_dir_all(layout.backup())?;
    let mut info = plist::Dictionary::new();
    info.insert("Unique Identifier".into(), plist::Value::String("a".repeat(40)));
    let mut buf = Vec::new();
    plist::to_writer_binary(&mut buf, &plist::Value::Dictionary(info)).unwrap();
    std::fs::write(layout.backup().join("Info.plist"), &buf)?;

    let mut manifest = plist::Dictionary::new();
    let mut lockdown = plist::Dictionary::new();
    lockdown.insert("UniqueDeviceID".into(), plist::Value::String("a".repeat(40)));
    manifest.insert("Lockdown".into(), plist::Value::Dictionary(lockdown));
    let mut mbuf = Vec::new();
    plist::to_writer_binary(&mut mbuf, &plist::Value::Dictionary(manifest)).unwrap();
    std::fs::write(layout.backup().join("Manifest.plist"), &mbuf)?;
    Ok(())
}

#[test]
fn exec_plan_rewrites_identity_only_for_legacy() {
    use bypass::{exec, Route, TaskLayout};

    let device = sample_device();
    let root = std::env::temp_dir().join(format!("atk-execplan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    // 旧路线：应当改写
    let legacy = TaskLayout::new(&root, &device.udid);
    seed_backup(&legacy).expect("造备份");
    let plan = exec::plan(&device, Route::LegacyGaval, &root, None, None).expect("计划");
    assert_eq!(plan.route, Route::LegacyGaval);
    assert!(plan.notes.iter().any(|n| n.contains("Info.plist")), "{:?}", plan.notes);
    let data = std::fs::read(legacy.backup().join("Info.plist")).unwrap();
    let value = plist::Value::from_reader(std::io::Cursor::new(&data)).unwrap();
    assert_eq!(
        value
            .as_dictionary()
            .and_then(|d| d.get("Unique Identifier"))
            .and_then(plist::Value::as_string),
        Some("00008110-000A1C2E3F40001E")
    );

    // 现代路线：备份文件在**任务根目录**（cwd 就是那里），且保留原始字节
    let root2 = std::env::temp_dir().join(format!("atk-execplan2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root2);
    std::fs::create_dir_all(&root2).unwrap();
    for name in bypass::pipeline::MODERN_REQUIRED_FILES {
        let body: &[u8] = if name == "Manifest.plist" {
            b"bplist00-manifest"
        } else {
            b"x"
        };
        std::fs::write(root2.join(name), body).unwrap();
    }
    let before = std::fs::read(root2.join("Manifest.plist")).unwrap();
    let plan = exec::plan(&device, Route::Mdm27, &root2, None, None).expect("计划");
    let after = std::fs::read(root2.join("Manifest.plist")).unwrap();
    assert_eq!(before, after, "现代路线不得改写备份字节");
    assert!(
        plan.notes.iter().any(|n| n.contains("不改写")),
        "{:?}",
        plan.notes
    );

    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&root2);
}

#[test]
fn exec_apply_twice_and_judges() {
    use bypass::{exec, pipeline, Route, TaskLayout};

    let device = sample_device();
    let root = std::env::temp_dir().join(format!("atk-execapply-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let layout = TaskLayout::new(&root, &device.udid);
    seed_backup(&layout).expect("造备份");

    let plan = exec::Plan {
        route: Route::LegacyGaval,
        layout: layout.clone(),
        command: Some(pipeline::legacy_restore(&layout, "MDM_SETUP.exe")),
        notes: Vec::new(),
    };
    let mut rounds = Vec::new();
    let outcome = exec::apply(
        &plan,
        |_cmd, round| {
            rounds.push(round);
            Ok("First restore complete, waiting for device restart\n\
                Second restore complete, waiting for final restart\n\
                IKEY MDM Successful."
                .into())
        },
        |_| Ok(()),
    )
    .expect("应完成");
    assert_eq!(rounds, vec![1, 2]);
    assert!(outcome.is_success());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn exec_extract_zip_rejects_non_zip() {
    use bypass::exec;

    let dir = std::env::temp_dir().join(format!("atk-badzip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fake = dir.join("x.zip");
    std::fs::write(&fake, b"definitely not a zip").unwrap();
    let err = exec::extract_zip(&fake, &dir.join("out")).unwrap_err();
    assert!(err.contains("压缩包") || err.contains("读取"), "实际：{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

// 注：原来这里有两个「缺文件时报错」的集成测试（缺恢复器、缺现代组件）。
// 资源已改为编译期内置，`plan` 会自动写入，用户不需要准备任何文件，
// 因此这两个断言不再成立；对应的验证在 `exec` 与 `resources` 的单元测试里。

#[test]
fn exec_prepare_needs_no_external_files() {
    use bypass::exec;

    let dir = std::env::temp_dir().join(format!("atk-prepare-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let text = exec::handle(
        "bypass-prepare",
        &sample_device(),
        &dir.display().to_string(),
        "",
    )
    .expect("不应要求用户准备文件");
    assert!(text.contains("内置资源"), "报告应说明用了内置资源：{text}");
    assert!(dir.join("ikey").join("MDM_SETUP.exe").is_file());
    let _ = std::fs::remove_dir_all(&dir);
}

/// 找从 `iFineMDM.exe` 里挖出来的 legacy-kit。
///
/// 用 `tools/carve-legacy-kit.py` 生成；没有就跳过（不伪造真实资源）。
fn legacy_kit() -> Option<std::path::PathBuf> {
    if let Ok(value) = std::env::var("ATK_LEGACY_KIT") {
        let path = std::path::PathBuf::from(value);
        if path.join("GavalLegacyInfo.plist").is_file() {
            return Some(path);
        }
    }
    for candidate in ["../legacy-kit", "../../legacy-kit", "legacy-kit"] {
        let path = std::path::PathBuf::from(candidate);
        if path.join("GavalLegacyInfo.plist").is_file() {
            return Some(path);
        }
    }
    None
}

#[test]
fn install_kit_rekeys_the_real_vendor_templates() {
    use bypass::{exec, Route, TaskLayout};

    let Some(kit) = legacy_kit() else {
        eprintln!("跳过：未找到 legacy-kit（用 tools/carve-legacy-kit.py 生成）");
        return;
    };

    let device = sample_device();
    let root = std::env::temp_dir().join(format!("atk-realkit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);

    let plan = exec::plan(&device, Route::LegacyGaval, &root, Some(&kit), None)
        .expect("应当能装配真实 kit");

    // 1) 恢复器进了 ikey/（cwd 约定的位置）
    let restorer = TaskLayout::new(&root, &device.udid)
        .ikey()
        .join("MDM_SETUP.exe");
    assert!(restorer.is_file(), "MDM_SETUP.exe 应被装入 ikey/");
    assert_eq!(
        std::fs::metadata(&restorer).unwrap().len(),
        5_494_784,
        "恢复器体积应与内嵌资源一致"
    );

    // 2) 备份装进了 ikey/<UDID>/
    let layout = TaskLayout::new(&root, &device.udid);
    assert!(layout.backup().join("Manifest.db").is_file());

    // 3) Info.plist：模板令牌必须全部替换掉
    let info_bytes = std::fs::read(layout.backup().join("Info.plist")).expect("Info.plist");
    let info_text = String::from_utf8_lossy(&info_bytes).to_string();
    assert!(
        !info_text.contains("__GAVAL_"),
        "模板令牌必须被替换，残留：{}",
        &info_text[..info_text.len().min(400)]
    );
    let info = plist::Value::from_reader(std::io::Cursor::new(&info_bytes)).unwrap();
    let dict = info.as_dictionary().unwrap();
    assert_eq!(
        dict.get("Unique Identifier").and_then(plist::Value::as_string),
        Some(device.udid.as_str())
    );
    assert_eq!(
        dict.get("Serial Number").and_then(plist::Value::as_string),
        Some(device.serial.as_str())
    );

    // 4) Manifest.plist：Lockdown 改写，但加密材料必须原样保留
    let man_bytes = std::fs::read(layout.backup().join("Manifest.plist")).expect("Manifest.plist");
    let man = plist::Value::from_reader(std::io::Cursor::new(&man_bytes)).unwrap();
    let root_dict = man.as_dictionary().unwrap();
    let lockdown = root_dict
        .get("Lockdown")
        .and_then(plist::Value::as_dictionary)
        .expect("Lockdown");
    assert_eq!(
        lockdown.get("UniqueDeviceID").and_then(plist::Value::as_string),
        Some(device.udid.as_str())
    );
    assert_eq!(
        root_dict.get("IsEncrypted").and_then(plist::Value::as_boolean),
        Some(true),
        "IsEncrypted 不能被改动"
    );
    assert_eq!(
        root_dict
            .get("BackupKeyBag")
            .and_then(plist::Value::as_data)
            .map(|d| d.len()),
        Some(1388),
        "BackupKeyBag 必须逐字节保留"
    );

    assert!(plan.notes.iter().any(|n| n.contains("装入工具")));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn payload_selection_is_by_minos_not_name() {
    // bypass3 是 12.4、bypass2 是 13.3 —— 按文件名序号会选错
    let p = payloads::pick_for_ios("12.4").expect("应有匹配");
    assert_eq!(p.minos, "12.4.0");
    let p = payloads::pick_for_ios("17.5.1").expect("应有匹配");
    assert_eq!(p.minos, "15.5.0");
}

#[test]
fn backend_serves_the_full_protocol() {
    let secrets = sample_secrets();

    // 造一个最小的 8 文件等价模板（数量不必是 8，覆盖多层级即可）
    let template = std::env::temp_dir().join(format!("atk-e2e-tpl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&template);
    std::fs::create_dir_all(template.join("sub")).expect("建目录");
    std::fs::write(template.join("Info.plist"), b"bplist00-info").expect("写文件");
    std::fs::write(template.join("Manifest.plist"), b"bplist00-manifest").expect("写文件");
    std::fs::write(template.join("sub/blob.bin"), [0u8, 1, 2]).expect("写文件");

    let mut config = backend::Config::new(secrets.clone());
    config.template_dir = Some(template.clone());

    // 1) 版本检查：用同一把密钥解回来
    let version = backend::respond(&config, "GET", "/A12/mdm/win_mdm.php", &BTreeMap::new(), b"");
    assert_eq!(version.status, 200);
    let value: serde_json::Value = serde_json::from_slice(&version.body).expect("JSON");
    let venv = crypto::VersionEnvelope::new(secrets.version_key, secrets.version_iv);
    let now = value["timestamp"].as_i64().expect("timestamp");
    let inner = venv.open(&value, now).expect("解版本信封");
    let doc: serde_json::Value =
        serde_json::from_str(inner["data"].as_str().expect("data")).expect("内层 JSON");
    assert_eq!(doc["version"], "1.0.0");

    // 2) 授权
    let auth = backend::respond(
        &config,
        "GET",
        "/A12/mdm/api1.php?serialNumber=M69PCG7LJX",
        &BTreeMap::new(),
        b"",
    );
    assert_eq!(auth.status, 200);
    assert!(String::from_utf8_lossy(&auth.body).contains("AUTHORIZED"));

    // 3) 备份源：带 nonce 才能出信封
    let nonce = [0x2au8; 16];
    let mut headers = BTreeMap::new();
    headers.insert(
        "x-aldaz-nonce".to_string(),
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>(),
    );
    let backup = backend::respond(&config, "POST", "/A12/mdm/backup.php", &headers, b"{}");
    assert_eq!(backup.status, 200);
    let envelope_json: serde_json::Value = serde_json::from_slice(&backup.body).expect("JSON");
    let envelope = crypto::Envelope::from_json(&envelope_json).expect("信封");
    assert!(envelope.verify_mac(&secrets.backup_secret, &nonce));
    let plain = envelope
        .open(&secrets.backup_secret, &nonce)
        .expect("解备份信封");
    let payload: serde_json::Value = serde_json::from_slice(&plain).expect("JSON");
    assert_eq!(payload["password"], backend::BACKUP_PASSWORD);
    assert_eq!(
        payload["files"].as_object().map(|m| m.len()),
        Some(3)
    );

    // 4) 激活
    let activate = backend::respond(
        &config,
        "POST",
        "/A12/mdm/amdeviceactivate.php",
        &BTreeMap::new(),
        b"{}",
    );
    assert_eq!(activate.status, 200);
    assert!(String::from_utf8_lossy(&activate.body).contains("true"));

    let _ = std::fs::remove_dir_all(&template);
}

#[test]
fn rekey_then_verify_identity_changed() {
    let dir = std::env::temp_dir().join(format!("atk-e2e-rekey-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建目录");

    // 用真实 plist 结构造模板，避免依赖外部文件
    let mut info = plist::Dictionary::new();
    info.insert("Unique Identifier".into(), plist::Value::String("a".repeat(40)));
    info.insert("Serial Number".into(), plist::Value::String("AAAAAAAAAAAA".into()));
    info.insert("Device Name".into(), plist::Value::String("AppleDevice".into()));
    let mut buf = Vec::new();
    plist::to_writer_binary(&mut buf, &plist::Value::Dictionary(info)).expect("序列化");
    std::fs::write(dir.join("Info.plist"), &buf).expect("写文件");

    let mut manifest_root = plist::Dictionary::new();
    let mut lockdown = plist::Dictionary::new();
    lockdown.insert("UniqueDeviceID".into(), plist::Value::String("a".repeat(40)));
    lockdown.insert("IsEncrypted".into(), plist::Value::Boolean(true));
    manifest_root.insert("Lockdown".into(), plist::Value::Dictionary(lockdown));
    let mut mbuf = Vec::new();
    plist::to_writer_binary(&mut mbuf, &plist::Value::Dictionary(manifest_root)).expect("序列化");
    std::fs::write(dir.join("Manifest.plist"), &mbuf).expect("写文件");

    let device = sample_device();
    let reports = rekey::rekey_backup_dir(&dir, &device).expect("重签发");
    assert_eq!(reports.len(), 2);

    let data = std::fs::read(dir.join("Info.plist")).expect("读回");
    let value = plist::Value::from_reader(std::io::Cursor::new(&data)).expect("解析");
    let dict = value.as_dictionary().expect("字典");
    assert_eq!(
        dict.get("Unique Identifier").and_then(plist::Value::as_string),
        Some("00008110-000A1C2E3F40001E")
    );

    let mdata = std::fs::read(dir.join("Manifest.plist")).expect("读回");
    let mvalue = plist::Value::from_reader(std::io::Cursor::new(&mdata)).expect("解析");
    let lockdown = mvalue
        .as_dictionary()
        .and_then(|d| d.get("Lockdown"))
        .and_then(plist::Value::as_dictionary)
        .expect("Lockdown");
    assert_eq!(
        lockdown.get("UniqueDeviceID").and_then(plist::Value::as_string),
        Some("00008110-000A1C2E3F40001E")
    );
    // 未触碰的键必须保留
    assert_eq!(
        mvalue
            .as_dictionary()
            .and_then(|d| d.get("Lockdown"))
            .and_then(plist::Value::as_dictionary)
            .and_then(|d| d.get("IsEncrypted"))
            .and_then(plist::Value::as_boolean),
        Some(true)
    );

    let _ = std::fs::remove_dir_all(&dir);
}
