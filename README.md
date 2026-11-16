# Unbound 0.8

Rust + egui/eframe 本地桌面设备管理程序。Windows 单文件发布为上级目录的 Unbound.exe；组件自动解压校验，不需要选择原始工具目录。

界面保留四个入口：设备总览、详细验机、备份与刷机、应用与文件。

详细验机报告只展示检测项目、实际读取值、读取说明。JSON 同样只含当前读数及来源，没有原值、原装判定或历史对比字段。读取不到的项目显示“未获取”。

当前支持设备信息、实时电量/存储、重启/关机、截图、崩溃记录、备份/恢复/复制、已知备份密码管理、固件检查、正常模式发起刷机、普通抹除、应用和共享文件管理。刷机/恢复/抹除/安装等写入操作需要在界面核对设备并确认，编译通过不等于每种操作已在每台设备验证。

已舍弃功能从任务注册表移除。旧 MDM 流程及媒体/激活/恢复模板资源不再打包；旧资料保存在工作目录历史归档，不在新版执行路径中。

## 构建

需要 Rust 工具链。Windows 需匹配工具链的 MSVC 或 MinGW 链接器；图标资源编译使用 Windows SDK rc.exe（MSVC）或 windres（GNU）。可通过 RC / WINDRES 指定工具位置。

```text
cargo run --release
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

已缓存依赖时可添加 --offline。当前 assets/runtime.zip 为准备好的 Windows 组件包；重新打包使用 scripts/repack-runtime.ps1 -Stage <组件目录>。

macOS 源码可编译；实际运行需要同级 tools 或 Homebrew 位置的原生 libimobiledevice/libplist/idevicebackup2/idevicerestore 等组件，不能使用 Windows DLL。没有进行 Mac 真机验证。

## 代码

- src/app.rs：四页导航、实际读数报告、后台任务管理。
- src/presentation.rs：iOS 浅色三栏界面、设备卡、搜索和快捷操作。
- src/design.rs：浅色 / 半透明表面、圆角、阴影、交互状态与矢量图标。
- src/connection.rs：独立 USB 连接检查，不反复读取完整设备信息。
- src/toolbox_ui.rs：保留功能的参数表单、应用/文件列表、实时状态。
- src/actions/：任务白名单、参数核对、设备及本地文件操作。
- src/mdm/：沿用历史模块名，只保留设备识别与只读详细信息工作进程。
- src/inspection/：当前硬件读数及 HTML/JSON 导出。
- src/bundled.rs：组件完整性校验与释放。

本次范围及实际验证结果见 VALIDATION.md 与上级 Unbound-使用说明.md。
应用标志源文件位于 assets/brand/；build.rs 将多尺寸 ICO 写入 Windows 可执行文件，窗口/任务栏与左上角使用同一图形。