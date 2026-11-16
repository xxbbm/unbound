# 安卓支持与平台切换（ANDROID.md）

本文件记录这一轮做的两件事：**逆向两个目标**的结论，以及把 Android 接进 Unbound 的实现。

---

## 1. 两个目标的逆向结论

### 1.1 `搞机助手 V4.9.1`（有用）

| 项 | 值 |
|---|---|
| 平台 | .NET Framework 4.0 / x86 / IL-only |
| 程序集 | `搞机助手` 4.9.1.0 |
| 规模 | 421 个 TypeDef、7,846 个 MethodDef、81 个内嵌资源、13 个 AssemblyRef |
| 随包 | `adb.exe`（5.99 MB）、`AdbWinApi.dll`、`AdbWinUsbApi.dll`、`AdbkillPort.exe` |
| SHA-256 | `d561d35b32101ca163f9007f3adee8328d53b3fed25fe9e983757bc6009b95a2` |

**元数据是残缺的**（`Module` 表出现 2 行、`Assembly` 表 2 行、`#GUID`/`#Blob` 索引越界），
字符串也被成对伪造 —— 这是 .NET 保护的特征，所以方法体级还原没有意义。

但从 ASCII 串能确认它**做什么**：

| 证据 | 能力 |
|---|---|
| 随包 `adb.exe`、`adb5037`、`Adbprot`/`get_Adbprot`/`set_Adbprot` | ADB 服务与端口处理 |
| `fastboot flash boot boot.img`、`boot.img`、`fastbootUSB30` | fastboot 刷写 |
| `reg add "HKLM\SYSTEM\CurrentControlSet\Control\usbflags\18D1D00D0100" /v osvc /t REG_BINARY /d "0000" /f` | 修 USB 描述符标志（`VID=18D1` Google + `PID=D00D` + `BCDDevice=0100`） |
| `classes.dex`（内嵌 ZIP 成员） | 随包 dex |
| `AesCryptoServiceProvider` / `RSACryptoServiceProvider` / `CreateDecryptor` | 自身资源的加解密 |

### 1.2 `紫罗兰工具箱 V3.7`（与安卓/iOS 无关）

| 项 | 值 |
|---|---|
| 外壳 | UPX 5.x（`UPX0`/`UPX1`/`.rsrc`，熵 7.98） |
| 脱壳后 | 5 节 / `.text` 1.1 MB / `.rsrc` 843 KB，压缩率仅 98.1% |
| 布局 | PE 区约 2.1 MB + **31.9 MB 加密 overlay**（无 ZIP/7z/NSIS/PE 任何签名） |

脱壳后导入表说明它是 **Windows PE 维护工具箱**，不是设备管理工具：

- `WIMGAPI.DLL`、`vhdmount.dll`、`VirtDisk.DLL`、`PECMD.exe` → WIM/VHD 映像挂载
- `SETUPAPI.dll` + 中文串「安装驱动:」「强制安装驱动:」「找到驱动:」「扫描硬件改动...」→ 驱动安装
- 「是否保留这些显示设置」「秒后转换为以前的显示设置」「位 刷新率」→ 显示模式修改并自动回滚
- `AdjustTokenPrivileges`、`CreateProcessWithLogonW`、`CreateProcessAsUserW` → 提权/换用户运行

**它里面没有安卓或 iOS 的功能可以搬。** 所以本轮只从 `搞机助手` 取能力。

---

## 2. Unbound 里的实现

### 2.1 新增 lib 模块

| 文件 | 职责 |
|---|---|
| `src/platform.rs` | 平台枚举、`adb devices -l` 解析、标识归属判定、自动/固定选择解析 |
| `src/android/mod.rs` | ADB / fastboot 命令构造、USB 标志修复命令、能力清单 |

**标识判定顺序很重要**：先比 iOS 规则（40 位 hex 或 `8+16`），再比 Android 规则
（4–32 位、字符集受限）。两条规则不重叠；**判不出来返回 `None`**，不猜 ——
猜错会把安卓机送进 iOS 的恢复流程。

**`adb devices` 的三种状态必须区分**：

| 状态 | 含义 | 可否操作 |
|---|---|---|
| `device` | 已授权 | 是 |
| `unauthorized` | 手机上还没点"允许 USB 调试" | 否 |
| `offline` | 连接异常，通常要 `adb kill-server` | 否 |

一律当"已连接"会让人在未授权状态下点安装然后失败。

### 2.2 界面

- 标题 `Unbound` 下方新增 **自动 / iOS / Android** 三段切换。
- 再下一行是动态说明，例如「自动识别：已连接 Android 设备」「已固定为 iOS 设备管理」。
- 手动选择优先于检测结果：安卓机插着不动时仍可切回 iOS 处理。

**平台为 Android 时，设备总览换成正安卓面板**（`overview()` 开头分流，**完全不动
iOS 那套布局代码**）：

```text
安卓设备
通过 adb 连接；请在手机上允许 USB 调试            [刷新设备信息]

R58M12345AB   已授权   SM_G973F
0123456789ABCDEF  待授权（请在手机上允许 USB 调试）

┌────────────────────────────────────┐
│ 机型    SM-G973F                    │
│ 系统    Android 12（API 31）         │
│ 架构    arm64-v8a                   │
│ 序列号  R58M12345AB                  │
│ 电池    87%（充电中） 31.0°C  4.102V  │
│ 存储    可用 59.6 GB / 共 110.1 GB    │
│ 屏幕    1080×2400  420 dpi          │
└────────────────────────────────────┘
```

拿不到的项显示「未获取」，**不填占位值**。

数据来源与解析：

| 项 | 命令 | 解析要点 |
|---|---|---|
| 机型/系统/API/架构/序列号 | `shell getprop` | `[key]: [value]` 格式，空值视为缺失 |
| 电池 | `shell dumpsys battery` | `temperature` 是十分之一摄氏度；任一电源为真即"充电中" |
| 存储 | `shell df -k /data` | 数值单位是 1K 块，×1024 换算 |
| 屏幕 | `shell wm size` / `wm density` | 有 `Override` 取覆盖值，否则取 `Physical` |

**不阻塞界面**：这几条 `adb shell` 在独立线程里跑（`AndroidProbe`），每帧 `poll`
取结果；同一时刻只允许一个探测在飞，切换设备或点「刷新设备信息」会丢弃在飞结果。

### 2.3 自动识别

`src/connection.rs` 新增 `AndroidMonitor`，与 iOS 的 `Monitor` **完全独立**：

- 各自 3 秒轮询，各自 5 秒超时，各自 64 KiB 输出上限；
- adb 没装 / 服务没起来 / 设备待授权，**都不会**让 iOS 侧判定断连；
- 查询失败返回 `None`、保留上一次结果 —— 不把"查不到"当成"设备拔了"。

自动模式的优先级：**有 iOS 就连 iOS，否则 Android**。这与既有"一台电脑一次只处理
一台设备"的假设一致，也避免插着的安卓机把 iOS 流程挤掉。

---

## 2.4 动作目录（已接入）

`catalog()` 新增「安卓设备」分组，共 12 个动作（动作总数 27 → 39）：

| id | 名称 | 输入 | 危险 |
|---|---|---|---|
| `android-devices` | 查看安卓设备 | — | 否 |
| `android-info` | 读取安卓设备信息 | — | 否 |
| `android-shell` | 执行 shell 命令 | 命令文本 | 否 |
| `android-install` | 安装 APK | APK 文件 | 是 |
| `android-apk-info` | 解析 APK | APK 文件 | 否 |
| `android-uninstall` | 卸载应用 | 包名 | 是 |
| `android-push` | 推送文件 | 电脑文件（目标路径填在输出框） | 是 |
| `android-pull` | 拉取文件 | 设备路径（保存目录填在输出框） | 否 |
| `android-reboot` | 重启安卓设备 | 目标：空 / bootloader / recovery | 是 |
| `android-fastboot-flash` | fastboot 刷写分区 | 镜像文件（分区名填在输出框） | 是 |
| `android-fastboot-devices` | 查看 fastboot 设备 | — | 否 |
| `android-usb-flags` | 修复 USB 描述符标志 | — | 是 |

### 工具定位

`find_program` 按顺序找 `adb` / `fastboot`：

1. 环境变量 `UNBOUND_ADB` / `UNBOUND_FASTBOOT`（用户指定，最优先）
2. 可执行文件同目录 → `tools/` → `platform-tools/`
3. 裸名字，交给 PATH 解析

找不到时不报错，返回裸名字 —— 真没有时执行阶段会给明确错误，比探测阶段静默失败好。

### 校验规则怎么接的

`Task::validate` 里只改一处：安卓动作**跳过 iOS 的 UDID 校验**，改用安卓规则 ——

* 序列号若填了，必须像安卓序列号（4–32 位受限字符集），否则报"请重新选择设备"；
* **危险动作要求必须指定具体设备**（序列号不能为空）。否则 adb 会自己挑一台，
  等于在不知道操作对象的情况下改设备；
* 其余通用校验原样保留：危险操作仍需输入序列号确认、参数类型仍需匹配。

### 执行

`worker_main` 里提前分流到 `android::run`，与 iOS 的 `run(&root, &task)` 并列。
`run` 走项目自带的 `runner`：固定 argv、有超时、有输出上限（2 MiB）。超时按动作区分：
安装/推拉 600 秒，shell 120 秒，其余 60 秒。

三个动作有专门处理：

* `android-usb-flags` 写两条注册表值，走 `reg.exe` 而非 `adb`，需要管理员权限；
* `android-info` 把 `getprop` 的输出解析成一句话摘要（机型 / 版本 / API / 架构 / 序列号），
  不把上千个 key 全倒给用户；
* `android-fastboot-flash` 按阶段表编排：`reboot bootloader` → 轮询 `fastboot devices`
  → `fastboot flash` → `fastboot reboot`。**设备没进 fastboot 就停，不刷写也不重启**。

---

## 2.5 APK 解析（离线）

APK 里的 `AndroidManifest.xml` 是**二进制 XML（AXML）**，不是文本。项目依赖里没有
AXML 解析器，为一个只读需求引库不值当，所以按格式规范自己走一遍 chunk 结构
（`src/android/apk.rs`）：

```text
RES_XML 头 → 字符串池（UTF-8 / UTF-16 两种编码都要处理）
          → 若干 start element → 按元素名收属性
```

读到的字段：

| 字段 | 来源 |
|---|---|
| 包名 | `manifest@package` |
| 版本名 / 版本号 | `manifest@versionName` / `manifest@versionCode` |
| 最低 / 目标 SDK | `uses-sdk@minSdkVersion` / `@targetSdkVersion` |
| 权限 | 每个 `uses-permission@name` |
| 应用名 | `manifest@label`，仅当它是字面量字符串 |

**不解析 `resources.arsc`**，所以 `android:label` 若是资源引用（`@0x7f…`，绝大多数
APK 都是），只能报"资源引用（未解析 resources.arsc）"，**不会编一个名字出来**。

健壮性：所有读取都做边界检查并返回 `Result`。畸形包、截断包必须报错，不能 panic ——
输入来自用户文件。清单解压后超过 8 MiB 直接拒绝（防压缩炸弹）。

对应动作 `android-apk-info`：选一个 APK 就能看到包名/版本/SDK/权限，**不连设备**。
装 APK 之前可以先跑它确认装的是什么。

---

## 3. 命令构造的约束

与项目既有约定一致：**固定 argv，不拼 shell 字符串**。

```rust
// shell 子命令整条作为单个参数传入，不拆分
shell_args(&device, "ls -l /sdcard | grep foo")
// → ["-s", "R58M12345AB", "shell", "ls -l /sdcard | grep foo"]

fastboot_flash_args(serial, "boot", Path::new("boot.img"))
// → ["-s", serial, "flash", "boot", "boot.img"]
```

USB 标志修复返回**参数数组**而不是拼好的命令行，调用方负责以 `reg` 为程序名执行；
该操作需要管理员权限且会改注册表，界面上必须先征得同意。

---

## 4. 验证

```text
cargo test × 1 轮   : 163 passed / 0 failed / 1 ignored
cargo clippy -D warnings : exit 0
cargo build --release    : 通过，apple-toolkit.exe 45.8 MB
```

新增测试覆盖：`adb devices -l` 三态解析、属性提取、空/仅表头输入、平台规则不重叠、
自动与固定选择的优先级、ADB 各子命令的 argv 形状、USB 标志命令与逆向所得逐字一致；
动作入口方面覆盖 id 识别、`-s` 前缀只在该给时给、缺参数拒绝、包名防注入
（`-rf`、含空格）、重启目标白名单、usb-flags 走 `reg.exe`；
工具定位覆盖候选顺序（自带 → `tools/` → `platform-tools/` → PATH）；
设备信息覆盖 `getprop` 解析与空值不编造；
fastboot 覆盖 `fastboot devices` 过滤、阶段顺序、**设备没进 fastboot 时只执行第一步**、
镜像缺失时拒绝；
APK 解析覆盖**自造的二进制 AXML**（字符串池 + 三个元素）完整取值、摘要可读性、
非 AXML/空输入/截断输入不 panic、缺 package 时拒绝、label 为资源引用时不编造。

> 说明：APK 解析用合成 AXML 做测试，因为本机没有真实 APK 样本可当固件。测试数据
> 覆盖了 UTF-16 字符串池与三种属性类型（字符串 / 整数 / 引用）。

---

## 5. 明确没做

| 项 | 说明 |
|---|---|
| `resources.arsc` 解析 | 所以 APK 的应用名多为"资源引用"，显示不出真实名称 |
| 紫罗兰的任何功能 | 它是 Windows PE 维护工具，与本产品无关 |
| 真机验证 | 未连接任何安卓设备；面板排版只在编译层面被检查过 |
