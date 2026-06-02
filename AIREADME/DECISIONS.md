# DECISIONS — student-kpscan
<!-- ADR，append-only，只追加不改历史。运行时事故→MEMORY。 -->

## ADR-001 · 识别走 newapi 网关 vision，不直连厂商 · 2026-06-01
- Problem: 识别错题图需要 vision 模型，可选直连厂商 SDK 或经统一网关。
- Constraint: 已有自有的 OpenAI 兼容 newapi 网关；要可切换上游、不在客户端散落多家 key。
- Decision: 统一调 newapi 网关的 OpenAI 兼容端点（背后挂豆包 vision / Qwen-VL），单一可切换端点。
- Alternatives（否决）: 直连豆包 / Qwen 厂商 SDK（多 key、难切换、客户端暴露厂商 key）。
- Tradeoff: 多一跳网关 + 依赖网关侧渠道就位（当前 vision chat 渠道未接，见 RELATIONS）。

## ADR-002 · 知识点库大模型生成 v0 起步，不等考纲 · 2026-06-01
- Problem: 归类需要知识点树，但需求方电子考纲未到位。
- Constraint: 不被需求方进度阻塞，spike 要立刻能跑。
- Decision: 先用大模型生成浙教版初中数学知识点树 v0（章 / 节 / 点级），考纲电子版到位后校准替换。
- Alternatives（否决）: 等考纲再开工（阻塞 spike）。
- Tradeoff: v0 有校准不确定项（已在 YAML 内 `# note` 标注），归类准确率天花板隐性。

## ADR-003 · MVP 只做诊断报告，不判对错 / 不推荐 / 只数学 · 2026-06-01
- Problem: 需求方期望多，1 周 MVP 容易膨胀。
- Constraint: 1 周交付、Windows、风险共担、分阶段投入。
- Decision: MVP 锁定 = 只传错题图 + 只数学 + 只出诊断报告（知识点薄弱点排序 + 错题数 + 可导出）。不判对错、不检红叉、不推荐、不做其他学科 / 移动端。
- Alternatives（否决）: 含对错判断 / 红叉检测 / 推荐练习（需题库 + 检测模型，超 1 周）。
- Tradeoff: 家长需自己挑错题（输入摩擦），自动检测 + 推荐放迭代 1。

## ADR-004 · 先做识别率 spike 作为开发 gating · 2026-06-01
- Problem: 整条链路唯一硬不确定性 = 数学公式 / 几何图识别 + 知识点归类准不准。
- Constraint: 未验证识别率前不应承诺 1 周 MVP。
- Decision: 开发前先做识别率 spike（纯调 API 脚本，测读题 + 归类两准确率），通过才进 MVP。
- Alternatives（否决）: 直接进 MVP 开发（识别不准则整产品不成立，返工成本高）。
- Tradeoff: 多一步前置，依赖真实样本 + 人工 ground truth + 通过线共识（见 ROADMAP Now）。

## ADR-005 · 桌面用 Tauri，Windows / 禁 360 · 2026-06-01
- Problem: 需交付到目标 Windows 办公电脑的桌面工具。
- Constraint: 体积小、相对少触发杀软误报；目标环境禁装 360。
- Decision: 用 Tauri（Rust + 前端）打桌面 app，目标 Windows，提前告知 IT 禁装 360。
- Alternatives（否决）: Electron（体积大、更易误报）。
- Tradeoff: Tauri 生态 / 打包签名相对 Electron 资料少，Windows 部署 + 杀软签名仍是已知坑。

## ADR-006 · 知识点 ID 稳定不可变（校准只改 name）· 2026-06-01
- Problem: 知识点树要随考纲校准演进，但统计按知识点聚合。
- Constraint: 历史错题统计的聚合键必须稳定。
- Decision: 知识点 ID（`册.章.节.点`）分配后不可变；校准只在原地改 name，结构大改才 bump 版本。
- Alternatives（否决）: 校准时重排 / 重编 ID（导致历史统计断裂）。
- Tradeoff: ID 可能与最终考纲编号不完全对齐（用 name 承载可读语义弥补）。

## ADR-007 · 不区分错误类型，错了即该知识点薄弱 · 2026-06-01
- Problem: 原 PRD 想区分粗心 / 审题错误 / 概念不清，错误类型分类逻辑偏复杂。
- Constraint: MVP 要简单可落地；无对错判断能力，错误类型判定本身不可靠。
- Decision: 不区分错误类型，某知识点错了即判定该知识点薄弱，只统计「哪个知识点错了多少题」。
- Alternatives（否决）: 按错误类型细分（原 PRD 思路，复杂且判定不可靠）。
- Tradeoff: 损失「为什么错」的颗粒度，换取可落地 + 一致的薄弱度量。
