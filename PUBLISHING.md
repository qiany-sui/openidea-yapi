# 发布 Java YApi Sync

公开源码：[qiany-sui/openidea-yapi](https://github.com/qiany-sui/openidea-yapi)。
插件 ID 为 `nc.yapi`，发布者为已登记的 `nc`。复用仓库外保存的发布者密钥，不重新生成或上传私钥。

## 验证与签名

1. 在干净源码副本按 README 构建，运行完整 Rust/Java 测试、格式检查、clippy 和最新正式宿主的插件检查器。确认目标提交的 GitHub Actions 通过。
2. 核对版本、许可证、所需权限及平台验收范围。真实服务、token、用户路径和本机验收记录不得进入公开源码或发行包。
3. 将已有密钥路径通过本机环境变量 `YAPI_PUBLISHER_KEY` 提供给官方 CLI；变量内容是路径，私钥不进入命令参数或日志。

   ```sh
   mkdir -p dist/releases/0.2.0
   openidea-plugin pack . --key "$YAPI_PUBLISHER_KEY" --publisher nc --out dist/releases/0.2.0/nc.yapi-0.2.0.oiplugin
   openidea-plugin check dist/releases/0.2.0/nc.yapi-0.2.0.oiplugin
   ```

4. 验证签名的公钥与 `l0ng-ai/openidea-plugins` 中 `publishers/nc.toml` 一致。签名后不再覆盖或修改该包；普通构建和 CI 的未签名包保留在另一目录。
5. 从发行提交生成源码归档，包含 vendored SDK、ASM 原始依赖及许可证；不包含 `.scratch`、`target`、本机设置或密钥。
6. 计算最终 `.oiplugin` 与源码归档的 SHA-256，写入 `SHA256SUMS`。

## GitHub Release 与市场登记

创建对应 tag 和 Release，上传签名插件、源码归档及 `SHA256SUMS`。从公开下载地址重新下载并核对每个文件的摘要及签名。

向 [社区登记仓库](https://github.com/l0ng-ai/openidea-plugins) 提交 `plugins/nc.yapi.toml`，填写公开源码仓库、版本、签名包 URL 和最终 SHA-256；可使用 `suggest = ["pom.xml"]`。已有发布者无需重复登记或变更公钥。

等待登记 CI 的包检查、摘要及签名校验，并由维护者审核合并。合并后核对社区 `index` Release 与官方 OpenIDEA 的 `plugins-index.json`；只提交 PR 或发布 Release 不等同于已上架。后续更新保留旧宿主所需版本条目。

本机安装版本与公开发行版本分别记录。发布不会自动覆盖已安装插件或修改现有授权。
