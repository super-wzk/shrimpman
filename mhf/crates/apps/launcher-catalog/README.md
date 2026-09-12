# Launcher Mod 清单

应用层的内建 Mod 元数据，由 Launcher 和 Mod 管理器共用。两者通过 `builtin_catalog!()` 构建清单，
条目直接由调用方的 `#[cfg(feature = "...")]` 选择，没有 `base`、`login` 等运行时布尔开关。
`BuiltinCatalog` 只持有已注册条目，提供候选清单和默认启动项。
Config、DatRedirect 始终可用；DatRedirect 由独立开关启用。

清单中的 Mod 与外部包使用相同的 `Manifest`、版本和依赖规则。应用将这里的候选与发现的
外部包一起交给 `mhf-mod-package` 解析；解析器不识别 Base、Debug 等具体功能，也不对来源增加依赖限制。

此 crate 不构造或加载 Mod。实际内建实现由 Launcher 的 `builtins.rs` 工厂按 features 组装，
包括为同一次启动创建共享的 UI 注册表。通用宿主按候选来源调用工厂或加载 DLL。

Cargo features 决定实现是否编译进应用，运行配置决定是否选择该 Mod。
`login`、`debug`、`workbench` feature 均依赖 `base`；内建版本和依赖范围统一记录在此清单中。
条件编译在调用方展开，因此同一次 Cargo 构建中两个应用启用不同 features 时，清单仍分别对应各自的实现。
