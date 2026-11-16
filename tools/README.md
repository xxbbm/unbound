将当前平台的原生可执行文件及其依赖放在这里。

发布目录示例：

```
apple-toolkit.exe       # macOS 为 apple-toolkit
tools/
  idevice_id.exe
  ideviceinfo.exe
  idevicepair.exe
  idevicebackup2.exe
  toolkit-demo.exe
  ...动态库和资源...
```

这份源码不附带第三方商业工具或设备备份。
不同工具的同名依赖可能不兼容，建议各自保留完整原目录，再通过 GUI 指定可执行文件绝对路径。
