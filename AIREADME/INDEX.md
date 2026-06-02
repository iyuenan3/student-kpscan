# student-kpscan · AIREADME
> 初中错题 AI 诊断辅导工具（纯工具型，浙江中考 / 数学 MVP）｜ 生命周期: planned
> last-synced: 24b589b · 2026-06-02   <!-- update 靠它算 delta；INDEX 不列自己 -->

<!-- 路由器：只指路，不放实质内容。INDEX 不列自己。任何文件增减/状态变都更新这里。符号：✅已填 / ⚑占位 / —N/A -->

## 状态
| 文件 | 状态 | 摘要 |
|---|:--:|---|
| CORE | ✅ | 身份 / Non-Goals / 6 条红线（识别走 newapi、spike gating、未成年人数据、防膨胀、全角标点）|
| RELATIONS | ✅ | 出向依赖 newapi 网关（私有 OpenAI 兼容 vision）|
| SPEC | ⚑ | 识别输出 JSON 契约 + 知识点 ID 契约（待 spike 验证、vision 渠道待网关接入）|
| ARCHITECTURE | ✅ | 双层产品 + MVP 实现蓝图（Tauri 前后端 / SQLite schema / 染色聚合 / 复用 spike），代码待 spike 通过后开发 |
| DEPLOYMENT | ⚑ | Tauri / Windows / 禁 360 已定，未部署（当前仅 spike 本地 Python）|
| PRD | ✅ | 产品意图 + 产品形态（双层 / 知识树体检 / 错题本）+ 商业 / 关键风险 |
| ROADMAP | ✅ | Now = 第 0 步识别率 spike（gating）；Next = spike 过后按蓝图 Step 0~8 开发 |
| CONVENTIONS | ✅ | 知识点 ID 规则 / 全角标点 / key 不入库 / 校准规则 |
| DECISIONS | ✅ | 13 条 ADR（立项锁定 001~007 + MVP 产品与实现 008~013）|
| MEMORY | ✅ | .gitignore `.env.*` 误伤 `.env.example` |
| CHANGELOG | ✅ | v0 立项里程碑 |

## 按任务读
- 跨项目了解 → CORE + RELATIONS（+ SPEC 若要集成）
- 改架构 / 看 MVP 实现蓝图 → ARCHITECTURE + DECISIONS
- 部署 / 运维 → DEPLOYMENT
- 加功能 / 看产品形态 → PRD + ROADMAP + CONVENTIONS
- 跑 spike（当前焦点）→ ROADMAP + SPEC + 仓库 `spike/README.md`
