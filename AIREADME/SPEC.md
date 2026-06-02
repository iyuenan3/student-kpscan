# SPEC — student-kpscan
<!-- 对外契约：别人集成你需要的精确接口。不写实现(→ARCHITECTURE)/为何这么设计(→DECISIONS)。 -->

> 本项目是本地 Tauri 桌面工具，**不对外暴露 API / 端点**。本文件记两类对本项目稳定的契约：① 消费 newapi 的集成契约 ② 识别输出 + 知识点 ID 的内部数据契约（spike → MVP 共用）。⚑ 标记项待 spike 验证后定稿。

## 消费契约：调 newapi vision
- 端点：`POST {NEWAPI_BASE_URL}/v1/chat/completions`（OpenAI 兼容，base 由 `spike/.env` 配）。
- 鉴权：`Authorization: Bearer {NEWAPI_KEY}`（令牌从 `.env` 读，不入库）。
- 入参：messages 含 system prompt（喂展平的知识点树）+ user（文本指令 + 图片 `image_url` base64 data URI，`detail=high`）；`temperature=0`、`max_tokens=1500`。
- 端点 / 鉴权 / 模型清单由 newapi 网关部署方配置。⚑ vision chat 渠道待网关侧接入（豆包 vision / Qwen-VL）；若网关用自签证书，客户端需信任其 root CA（Python requests 需相应配置）。

## 识别输出契约（vision 返回 JSON，spike → MVP 稳定 schema）
模型按 system prompt 只输出 JSON：

```
{
  "read_text": "印刷体题目转写（LaTeX 表达公式）",
  "has_figure": true/false,
  "primary_kp_id": "主知识点 ID（必须取自知识点树现有 ID）",
  "primary_kp_name": "主知识点名",
  "alt_kp_ids": ["备选知识点 ID"],
  "confidence": 0.0-1.0,
  "reason": "归类依据"
}
```
- `primary_kp_id` 不得自造，必须命中知识点树现有 ID；不确定可截断到节 / 章级。
- ⚑ 字段集待 spike 跑通后定稿（可能据实际表现增删）。

## 知识点 ID 契约
- 格式：`册.章.节.点`（如 `8A.2.7.1`）。册码：`7A/7B/8A/8B/9A/9B`（七上 / 七下 / 八上 / 八下 / 九上 / 九下）。
- 统计聚合键 = `kp_id`。ID 分配后不可变（见 CONVENTIONS / DECISIONS-006）。
- 源文件：`knowledge/zhejiang-math-kp-v0.yaml`（v0：6 册 / 30 章 / 143 节 / 204 点）。

## 配额 / 分组
⚑ 取决于 newapi 网关渠道配置，待 vision 渠道配好后填。

## 版本 / 兼容
- 知识点树 v0 →（考纲电子版校准后）v1：**ID 保持稳定**，只改 name 不改 ID，保证历史统计不断裂。
