# RELATIONS — student-kpscan
<!-- 生态连接。出向用相对路径 ../<proj>/AIREADME/。共享底座写在属主项目根 AIREADME，这里只指向属主。 -->

## 出向依赖（我用了谁）
| 依赖 | 用途 | 部署 |
|---|---|---|
| newapi 网关 | 私有 OpenAI 兼容 LLM 网关，调其 vision 模型识别错题图 + 关联知识点 | 私有部署（不在本仓库）|

**依赖现状（关键路径风险）**：所用 newapi 网关当前未接入可用的 vision chat 渠道，故本项目 spike 的前置不只是填 `spike/.env`，而是**需先在网关侧接入一个 vision chat 渠道**（豆包 vision / Qwen-VL）。

## 入向（谁用我）
暂无。本地桌面工具，无下游消费方 / 无对外 API。

## 共享底座 / 复用资产
- **newapi 网关**：识别经一个私有 newapi 网关（OpenAI 兼容）。本项目是消费方，只在 `spike/.env` 配端点 + 令牌 + 模型名（不入库），不含网关本身的渠道 / 部署 / 证书配置。
- **知识点树 v0**（`knowledge/zhejiang-math-kp-v0.yaml`）= 本项目自有核心资产。
