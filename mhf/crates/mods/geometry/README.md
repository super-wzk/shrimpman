# Geometry

本领域维护 HD 客户端的几何扩展，没有独立公开服务接口。
`provider` 在 i686 Windows 上启用原生适配和 `GeometryMod::default()`，由
[`mhf-base`](../base/README.md) 始终组合运行，不作为独立 Mod 选择。

detach 移除几何补丁，`prepare_release` 归还额外游戏 DLL 引用；几何缓冲区仍由模块持有，
直到宿主完成游戏卸载。默认构建不链接原生实现，`cargo test -p mhf-geometry` 可独立验证
网格转换、三角形拓扑、设备缓存和补丁数据。
