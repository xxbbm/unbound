#!/usr/bin/env python3
"""从 iFineMDM.exe 里挖出旧路线的全部内嵌资源，组装成 legacy-kit。

为什么需要这个脚本：旧路线要用四份**内嵌资源**，它们不是独立文件，而是作为
.NET 托管资源串行化在 PE 里。直接找文件名只会命中元数据字符串堆，找不到内容。
这里按实际存储形态定位：

* `GavalLegacyTools.zip` / `GavalLegacyBackup.zip` —— 完整的 ZIP 中央目录结构，
  用 `PK\\x03\\x04 … PK\\x05\\x06` 定界，再用 `zipfile.testzip()` 校验。
* `GavalLegacyInfo.plist` / `GavalLegacyManifest.plist` —— **XML 文本**，
  用 `<?xml` … `</plist>` 定界（不是二进制 plist，所以搜 `bplist00` 会落空）。

输出布局与本项目 `bypass::exec::install_kit` 期望的一致：

    kit/
    ├─ GavalLegacyTools.zip
    ├─ GavalLegacyBackup.zip
    ├─ GavalLegacyInfo.plist
    ├─ GavalLegacyManifest.plist
    ├─ tools/     （解出的工具，含 MDM_SETUP.exe）
    └─ backup/    （解出的备份，含 Manifest.db / Status.plist / version.txt）

用法：
    python tools/carve-legacy-kit.py <iFineMDM.exe> <输出目录>
"""

from __future__ import annotations

import hashlib
import io
import plistlib
import sys
import zipfile
from pathlib import Path

LFH = b"PK\x03\x04"
EOCD = b"PK\x05\x06"

# 旧路线恢复器必须是这个名字，且与其 DLL 同目录（cwd 约定为 ikey/）
RESTORER = "MDM_SETUP.exe"

INFO_KEYS = (
    "Target Identifier",
    "Unique Identifier",
    "Serial Number",
    "Product Type",
    "Product Version",
    "Build Version",
    "Device Name",
    "Display Name",
)
LOCKDOWN_KEYS = (
    "UniqueDeviceID",
    "SerialNumber",
    "ProductType",
    "ProductVersion",
    "BuildVersion",
    "DeviceName",
    "IMEI",
)


def sha16(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()[:16]


def carve_zips(data: bytes) -> list[tuple[int, bytes, list[str]]]:
    """切出所有通过校验的 ZIP。返回 (offset, blob, namelist)。"""
    out: list[tuple[int, bytes, list[str]]] = []
    pos = 0
    while True:
        start = data.find(LFH, pos)
        if start < 0:
            break
        end = data.find(EOCD, start)
        if end < 0:
            pos = start + 4
            continue
        end += 22
        if end + 2 <= len(data):
            comment_len = int.from_bytes(data[end - 2 : end], "little")
            if 0 <= comment_len <= 0xFFFF:
                end += comment_len
        blob = data[start:end]
        try:
            with zipfile.ZipFile(io.BytesIO(blob)) as archive:
                if archive.testzip() is None:
                    out.append((start, blob, archive.namelist()))
        except Exception:
            pass
        pos = start + 4
    return out


def carve_plists(data: bytes) -> list[tuple[int, bytes, dict]]:
    """切出所有能解析的 XML plist。"""
    out: list[tuple[int, bytes, dict]] = []
    pos = 0
    while True:
        start = data.find(b"<?xml", pos)
        if start < 0:
            break
        pos = start + 5
        close = data.find(b"</plist>", start)
        if close < 0:
            continue
        blob = data[start : close + 8]
        try:
            obj = plistlib.loads(blob)
        except Exception:
            continue
        if isinstance(obj, dict) and len(obj) > 1:
            out.append((start, blob, obj))
    return out


def classify_plist(obj: dict) -> str | None:
    if any(key in obj for key in ("Unique Identifier", "Target Identifier", "Product Type")):
        return "Info.plist"
    lockdown = obj.get("Lockdown")
    if isinstance(lockdown, dict) and any(key in lockdown for key in LOCKDOWN_KEYS):
        return "Manifest.plist"
    return None


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    image = Path(sys.argv[1])
    kit = Path(sys.argv[2])
    if not image.is_file():
        print(f"找不到镜像：{image}")
        return 2

    data = image.read_bytes()
    print(f"镜像 {image.name}：{len(data):,} 字节，sha256={sha16(data)}")
    kit.mkdir(parents=True, exist_ok=True)

    # ---- ZIP ----
    zips = carve_zips(data)
    print(f"\n切出 {len(zips)} 个可校验 ZIP")
    tools_zip = backup_zip = None
    for offset, blob, names in zips:
        has_restorer = any(Path(n).name.lower() == RESTORER.lower() for n in names)
        looks_like_backup = any(n == "Manifest.db" or n.endswith("Status.plist") for n in names)
        label = "tools" if has_restorer else "backup" if looks_like_backup else None
        print(f"  @0x{offset:X}  {len(blob):>12,}B  {len(names):>4} 项  {sha16(blob)}  -> {label or '未识别'}")
        if label == "tools" and tools_zip is None:
            tools_zip = blob
        elif label == "backup" and backup_zip is None:
            backup_zip = blob

    if tools_zip is None or backup_zip is None:
        print("\n缺资源：未同时找到含恢复器的工具包与备份包，无法继续")
        return 1

    (kit / "GavalLegacyTools.zip").write_bytes(tools_zip)
    (kit / "GavalLegacyBackup.zip").write_bytes(backup_zip)

    # ---- plist ----
    plists = carve_plists(data)
    print(f"\n切出 {len(plists)} 个 XML plist")
    written: set[str] = set()
    for offset, blob, obj in plists:
        kind = classify_plist(obj)
        if kind is None or kind in written:
            continue
        written.add(kind)
        target = kit / ("GavalLegacy" + kind)
        target.write_bytes(blob)
        print(f"  @0x{offset:X}  {len(blob):>8,}B  {kind:16} 键 {len(obj)}  -> {target.name}")

    if written != {"Info.plist", "Manifest.plist"}:
        print(f"\n注意：只找到 {sorted(written)}；缺的模板会让备份缺少设备信息")

    # ---- 解出 ----
    for archive, target in (("GavalLegacyTools.zip", "tools"), ("GavalLegacyBackup.zip", "backup")):
        source = kit / archive
        dest = kit / target
        with zipfile.ZipFile(source) as zf:
            for info in zf.infolist():
                name = Path(info.filename)
                if name.is_absolute() or ".." in name.parts:
                    print(f"  拒绝越界路径：{info.filename}")
                    return 1
            zf.extractall(dest)
        count = sum(1 for p in dest.rglob("*") if p.is_file())
        print(f"  解出 {archive} -> {target}/（{count} 个文件）")

    # ---- 自检 ----
    print("\n自检：")
    restorer = kit / "tools" / RESTORER
    print(f"  恢复器 {RESTORER}: {'有' if restorer.is_file() else '缺'}"
          + (f"（{restorer.stat().st_size:,} B）" if restorer.is_file() else ""))
    sign = kit / "backup" / "version.txt"
    if sign.is_file():
        print(f"  version.txt = {sign.read_text(encoding='utf-8', errors='replace').strip()!r}")
    manifest = kit / "backup" / "Manifest.db"
    print(f"  Manifest.db: {'有' if manifest.is_file() else '缺'}")

    info_path = kit / "GavalLegacyInfo.plist"
    if info_path.is_file():
        obj = plistlib.loads(info_path.read_bytes())
        present = [k for k in INFO_KEYS if k in obj]
        tokens = sorted({v for v in obj.values() if isinstance(v, str) and v.startswith("__GAVAL_")})
        print(f"  Info 模板：{len(present)}/8 键在位，占位符 {len(tokens)} 个")
        for token in tokens:
            print(f"    {token}")

    man_path = kit / "GavalLegacyManifest.plist"
    if man_path.is_file():
        obj = plistlib.loads(man_path.read_bytes())
        lockdown = obj.get("Lockdown") or {}
        present = [k for k in LOCKDOWN_KEYS if k in lockdown]
        print(f"  Manifest：Lockdown {len(present)}/7 键；"
              f"IsEncrypted={obj.get('IsEncrypted')}；"
              f"BackupKeyBag={len(obj.get('BackupKeyBag') or b'')} B")

    print(f"\nkit 就绪：{kit}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
