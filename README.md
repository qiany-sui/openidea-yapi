# Java YApi Sync

从 Java Spring Controller 预览并同步接口到标准 YApi。独立插件 `nc.yapi`，无需修改 OpenIDEA 或改造 YApi，依赖类型读取器随单个插件包分发。当前版本 0.2.0，最低宿主插件 API **1.8**（OpenIDEA v0.1.45）。

[源码](https://github.com/qiany-sui/openidea-yapi) · [发行包](https://github.com/qiany-sui/openidea-yapi/releases) · [构建检查](https://github.com/qiany-sui/openidea-yapi/actions/workflows/ci.yml)

面向 **Java / Spring MVC**，本轮外部依赖解析支持 **Maven**；不适用于 Kotlin、Gradle 依赖或其他语言框架。

这是源码与字节码元数据解析插件，不依赖 IntelliJ PSI，不保证与 EasyYapi 的全部解析能力一致。参考其使用流程，没有复制其实现。

## 安装与使用

1. 从 [GitHub Releases](https://github.com/qiany-sui/openidea-yapi/releases) 下载 `nc.yapi-0.2.0.oiplugin`，在 OpenIDEA 插件设置中选择 **从插件包文件安装…**。正式发行包由 `nc` 发布者签名，下载后可用附件 `SHA256SUMS` 核验。市场审核及索引同步完成后，也可在插件市场搜索 **Java YApi Sync** 安装；Release 发布和市场上架是两个独立步骤。
2. 安装 curl，并在 OpenIDEA 的正常权限界面为 `process:curl` 授权实际可执行文件。插件按程序名 `curl` 调用，不写死系统路径，也不经过 shell；Windows 应选择 `curl.exe`，不是 PowerShell 的命令别名。macOS、Linux 选择本机实际的 curl。无需凭据管理工具。
3. 如 DTO 或 `ApiResponse<T>` 来自外部依赖，安装 JDK 17+ 与 Maven，在插件权限页分别授权 `process:java`、`process:mvn`。Java 元数据读取器已包含在插件内，不需要安装辅助服务。Maven 必须能使用本机已有依赖（含 `maven-dependency-plugin:3.8.1` 与 `maven-help-plugin`）；缺少缓存时先在正常 Maven 环境下载。本插件以离线模式运行，不自动拉取依赖。自定义 `settings.xml` 或本地仓库可在面板的「Maven 依赖解析设置」填写路径。
4. 打开并信任 Java 项目，在 Controller 编辑器或项目树的 Java 文件上执行 **YApi：预览并同步 Controller**。支持多选文件；不递归同步选中的目录。
5. 填写 YApi 服务地址（例如 `http://yapi.intranet:3000`，允许服务部署子路径）和 **YApi 项目 token**。从 YApi 项目「设置 → token」获取，不是个人登录 token。
6. 点击 **生成同步预览**。项目 ID 由 token 对应的 YApi 项目获取，在预览里显示项目名称、ID、接口和待写入字段。
7. 勾选接口，点击 **同步选中接口…**，在原生确认框核对目标后同步。再次同步相同内容显示「未变化」。取消不会写入。

当前编辑器未保存的源码参与解析。预览后源码或远端文档改变会要求重新预览。YApi 没有条件更新接口，检查与写入之间仍有短暂并发窗口；不要同时编辑同一接口。网络超时不自动重试，结果不明时重新预览，可避免盲目重复新增。

0.1.2 修复解析失败时面板仍显示未选择 Controller 或旧状态的问题。命令会先记录当前选择并使旧预览失效，再显示解析结果；请求体和响应体的类型错误会标明 Controller、方法、接口路径及失败字段。直接点击预览但尚未选择文件时，会尝试使用当前 Java 编辑器。解析失败时不请求 YApi，不生成可同步的部分预览。

更新以 **YApi 项目 + HTTP 方法 + 路径** 匹配，保留原 ID、分类、状态、tag 和手工备注。请求参数、请求体、响应体由源码生成；同名字段的示例、mock 和源码未提供的描述尽量保留。新增接口按 Controller 的首行注释或类名建立分类。不会删除远端接口；源码删除或改名后的旧接口需自行处理。

## 权限与凭据

遵循 [官方插件 API](https://github.com/l0ng-ai/openidea/blob/v0.1.45/plugins/README.md) 和 [社区登记规则](https://github.com/l0ng-ai/openidea-plugins#rules)，所有能力均在清单中声明并通过宿主授权，不手工修改授权文件。

| 能力 | 原因 |
| --- | --- |
| `project.read` | 读取所选 Controller 和项目内 DTO 源码；每次最多 5000 个 Java 文件、总计 20 MiB，单文件最多 1 MiB |
| `editor.read` | 获取当前 Controller 文件入口 |
| `storage` | 按本地项目保存 YApi 地址与 Maven 路径，不保存 token 或 settings.xml 内容 |
| `process:curl` | 向用户配置的 YApi 发起 HTTP(S) 请求，兼容内网 HTTP |
| `process:mvn`（可选） | 外部类型缺失时读取当前 Maven 模块依赖树，支持多模块 reactor，不执行 compile/package 生命周期 |
| `process:java`（可选） | 运行包内读取器，以 ASM 提取 JAR 的字段、泛型、注解和 getter 签名；不加载或执行业务类 |

官方动态网络授权不接受任意内网 HTTP；内置 secrets 仅支持请求头替换，而标准 YApi 从 query 或 JSON body 接受 token。因此使用官方授权的系统进程能力。curl 的 token 和 JSON 通过 stdin 传入，不使用 shell，不把 token 放入进程 argv；禁用默认 curl 配置、不跟随重定向、不关闭 TLS 校验。插件不会输出服务端原始错误正文、curl stderr 或 token。

HTTP 是明文传输，只应在你认可的内网使用。GET 的 query token 符合标准 YApi 协议，服务端及代理应避免记录敏感 query。插件无后台上传，只在预览和确认同步时连接指定服务。

所有平台统一使用会话 token，仅保存在当前插件 WASM 实例的内存中，不保存到磁盘或系统凭据库。重启、重载或实例空闲回收后需重新输入；同一实例内可留空复用。token 按本地项目及 YApi 地址隔离，切换地址时清空旧 token。点击「清除会话 token」会立即清除当前项目的内存 token 并使预览失效。重新打开项目或提示未选择 Controller 时，先在文件上重新执行同步命令。

同一个 WASM 包使用官方 SDK 的进程接口，支持配置各平台的 curl；但仍要求本机存在可用的 curl 并正常授权。0.2.0 已在 macOS ARM64、OpenIDEA 0.1.45 完成桌面安装、Java/Maven 授权、解析、预览及重复同步验收。Windows、Linux 尚未完成桌面实机验收，不将相同代码路径视作三平台测试通过。

## 解析范围

支持：

- Java Spring MVC 的 `Get/Post/Put/Delete/PatchMapping`、明确指定 HTTP 方法的 `RequestMapping`，字面量路径及路径数组。
- path/query/header、JSON `RequestBody`；简单 DTO 查询参数展开。
- 基本类型、数组、List/Set、String 键的 Map、Object 动态 JSON、项目内 DTO 与 record、简单泛型包装类；ResponseEntity/HttpEntity 的响应正文。
- Maven 已解析版本对应的依赖 JAR 类型，按需提取字段、泛型及公开 getter；例如外部 `ApiResponse<YourDto>` 的 `data` 使用真实 DTO 展开，不硬编码包装字段。项目源码优先。
- Javadoc、Swagger 常见说明，NotNull/NotEmpty/NotBlank、JsonProperty 重命名和 JsonIgnore；JsonCreator 属性构造参数的 required、常见 JsonInclude。计算 getter 仅进入响应模型。

`Object` 对应不限制 JSON 类型的 schema `{}`，`Map<String,Object>` 对应允许任意值的对象；不会臆造动态键。依赖读取器不调用静态初始化、构造器或 getter。Maven 使用受信任项目的配置和扩展；本机缺少实际 JAR、类型歧义或无法表达的序列化规则仍需处理后再同步。每次同步会重新解析依赖，防止预览后版本变化。

以下结构会报错而不生成部分 schema：缺失或有歧义的 DTO、继承、enum、循环引用、泛型边界、不支持的 Jackson 序列化注解、文件上传/Cookie/复杂查询参数、动态路径或映射条件。不解析 Kotlin、运行时配置、ControllerAdvice、全局响应包装、自定义注解或自定义 Jackson 序列化器。Java 标识符解析目前限 ASCII；classpath 中同名类按依赖顺序首次命中，不模拟运行期类加载器。扫描源码不是运行期契约推导，预览时仍应核对实际接口行为。

## 构建、验证与分享

需要 JDK 17+、Python 3、Rust（含 `wasm32-wasip2` target）和与 v0.1.45 匹配的官方 `openidea-plugin` CLI。官方 SDK/WIT 已按 release 固定在 vendor，构建不依赖作者本机绝对路径。确保 PATH 中的 `java`、`javac` 为 JDK 17+；也可分别设置 `YAPI_TEST_JAVA` 和 `JAVAC` 的绝对路径。CI 使用 Rust 1.99.1、JDK 17 与 Python 3.11。

```sh
python3 scripts/build-java-helper.py
python3 tests/java_helper_test.py
cargo test --locked
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --target wasm32-wasip2
mkdir -p dist
cp target/wasm32-wasip2/release/openidea_yapi.wasm dist/plugin.wasm
openidea-plugin check .
openidea-plugin pack . --out dist/nc.yapi-0.2.0.oiplugin
openidea-plugin check dist/nc.yapi-0.2.0.oiplugin
```

HTTP 集成测试需要允许 127.0.0.1 临时监听，只访问进程内测试服务，使用虚构 token；不依赖真实 YApi。

0.2.0 已通过 52 项 Rust 测试、3 项真实 Java 编译/读取器测试、fmt、clippy、WASM 构建及官方包检查。独立审查发现的两处 Jackson 注解遗漏均已用失败回归复现并修复，包含真实 helper→parser 集成测试。

真实 Maven 多模块工程的两个 Controller 接口已通过生产解析链路生成模型，并在获授权的临时 YApi 项目完成新增、结构回读和再次预览「未变化」验证。随后在安装版 WASM 插件中完成 Java/Maven 正常授权，实际预览核对动态 Map/Object、外部泛型包装和 record 响应，重复同步结果为「新增 0，更新 0，未变化 2」；远端回读确认 schema 一致、ID 保留且无重复。此轮桌面测试未产生新增或更新，不将原生写入测试冒充桌面写入测试。测试不运行或修改业务服务；真实地址、token 和业务数据不进入仓库或发行包。

正式发行使用发布者私钥签名，具体步骤见 [PUBLISHING.md](PUBLISHING.md)：
`openidea-plugin pack . --key <仓库外的私钥路径> --publisher nc --out dist/nc.yapi-0.2.0.oiplugin`。
私钥和真实 token 不得放进源码、安装包或日志。最终须经过市场维护者审核，遵循规范并不表示已获官方审核。

协议依据：[YApi Controller](https://github.com/YMFE/yapi/blob/master/server/controllers/interface.js)、[鉴权](https://github.com/YMFE/yapi/blob/master/server/controllers/base.js)。解析组件来源及完整许可证见 NOTICE、LICENSE。
