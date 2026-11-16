# Java YApi Sync 协作约定

独立社区插件 `nc.yapi`，当前版本 `0.2.0`，面向 Java Spring MVC 与 Maven。
源码仓库：https://github.com/qiany-sui/openidea-yapi 。不修改宿主、YApi 服务端或业务源码。

## 实现与权限

- Rust 2024；SDK/WIT 固定为官方 OpenIDEA v0.1.45，插件 API `^1.8`，不随意修改 vendor。
- 使用官方命令、菜单、HTML 视图与进程授权。必需能力为 project.read/editor.read/storage/process:curl；外部类型解析可选 process:java/process:mvn。
- token 只保存在当前 WASM 实例，按本地项目及目标地址隔离；不写磁盘、日志、命令参数或系统凭据库。
- curl 通过 stdin 接收请求；不使用 shell、不跟随重定向、不关闭 TLS 校验。网络仅由明确的预览与同步命令触发。
- Maven 离线解析当前 Controller 模块的依赖树，不执行 compile/package。Java 17+ 读取器随包分发，以 ASM 提取元数据，不加载或执行业务类。
- 项目源码优先；动态 Object/Map 保留开放 schema，未知或不支持的结构明确报错，不猜测或生成部分文档。
- 同步按项目、HTTP 方法、路径匹配；预览确认后写入，保留 ID、分类、状态及手工字段，不删除接口。同步前重验源码和远端状态。
- 只验证声明过的平台。macOS ARM64 桌面已验收；Windows/Linux 不能仅凭共用代码宣称通过。

## 构建与验证

构建顺序和命令见 README。JDK 17+、Python 3.11、Rust 1.99.1（wasm32-wasip2），依赖锁定。

- 修改前先复现；修改后运行相关回归，交付前运行完整 Rust/Java 测试、fmt、clippy、WASM 构建及官方 check。
- `resources/java-types.jar` 由 `scripts/build-java-helper.py` 生成，不提交；在 cargo 构建或测试之前生成。ASM 原始 JAR 及校验和保留在 vendor/asm。
- HTTP 测试只使用回环临时服务与虚构 token，不连接真实业务服务。桌面测试使用临时 fixture；真实临时服务仅在用户明确授权范围内验证。
- src/java.rs、model.rs、spring.rs 继承已有来源注释；许可证与第三方 NOTICE 必须随源码和插件分发。
- 只改本任务所需文件，不清理无关改动；提交前检查 status/diff，提交格式 `[ai] <type>(<scope>): <中文说明>`。

## 发布

按 PUBLISHING.md 执行，复用 `nc` 发布者的仓库外密钥。源码、CI、Release 及市场登记都不包含本机记录、凭据或真实业务数据。发布和安装分别操作，未经用户要求不改本机授权。

发布检查顺序：

1. 公开源码及完整测试 → 验证干净源码可构建，目标提交 CI 通过。
2. 签名与发行附件 → 验证已验收 WASM 一致、发布者公钥和签名有效、公开下载摘要一致。
3. 市场登记 → 验证登记 CI 与维护者合并状态；合并后核对社区及官方市场索引。
