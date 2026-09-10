# Translation Mod

当前暂未接入 launcher、Base、Quest 或管理器的运行清单。代码与资源保留供独立开发，以下内容描述本 crate 自身实现。

默认层提供公共 `api`、稳定字符串 `Key`、`Translation` 借用接口，以及
`TranslationConfig` / `MissingTranslation`。`offline_quest` 的样例文本也在默认层，
离线任务预处理无需启动 Mod 或加载词典 provider。

`provider` 启用 `TranslationService`、`TranslationMod` 与内嵌词典。`TranslationMod::new()`／`Default`
不接收配置；prepare 绑定独立 `mhf.config`，注册空默认值的 `translation` 节并自行解析
`TranslationConfig`。空表按 `TranslationService::new(None)` 处理；非空配置验证后创建服务并发布不可变表。
消费者只借用表，其生命周期由宿主协调。`headers` 提供 `api::define_header` 给启动应用聚合。

- `src/api/`：Rust 与 C 共用的数据和 trait 定义，以及配置。
- `src/provider/service.rs`：公共接口的缓冲区写入与错误处理。
- `src/provider/dictionary.rs`：公共 Key 到词典索引的转换与查询；ordinal 始终是实现私有数据。
- `src/provider/module.rs`：提供方实例与注册生命周期。
- `locales/`：UTF-8 JSONL 字典源。
- `tools/`：保留已有译文的模板生成器。

build.rs 只在 `provider` 启用时读取 locale，并复用 Unicode 的唯一资源 catalog
解析源。产物是各自 `OUT_DIR` 下的 `translations.rs` 和 `translations.bin`，不会
复制资源布局或再生成 Unicode 的 resources.rs。缺失译文与显式空译文保持不同语义，
公共接口输出不带 NUL，原生资源层负责追加终止符并持有稳定内存。

详细格式与工具用法见 [locale 说明](locales/README.md)。
