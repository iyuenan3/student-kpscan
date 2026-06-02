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

## ADR-008 · 双层结构（知识树宏观体检 + 错题本微观可回看）· 2026-06-02
- Problem: 产品要同时服务家长（看大局、决策）和孩子（针对性复习），且要长期用。
- Constraint: 朴素诊断（错题 → 知识点），不做复杂掌握度推断。
- Decision: 双层 = 宏观「会染色的知识树体检」（主给家长）+ 微观「按知识点归类、可回看的错题本」（主给孩子），同一套数据两种用法。
- Alternatives（否决）: 只做一次性报告生成器（缺回看与长期价值）。
- Tradeoff: 比纯报告多一层错题本存储与浏览，但覆盖双角色 + 长期价值。

## ADR-009 · 一道错题点亮多个知识点 + 计数口径 · 2026-06-02
- Problem: 一道错题常涉及多个知识点，只算主考点会漏掉它暴露的其他知识点。
- Constraint: 朴素诊断「错了即关注它涉及的知识点」。
- Decision: 一题点亮 primary + alt 涉及的所有知识点各 +1。节点「涉及 N 道」按子树去重（HashSet，同题对同祖先只算 1，防父节点重复累加）；界面顶部另显「共 X 道」真实错题总数。统计层主 / 次一视同仁，详情页用 role 区分主考点 vs 涉及考点。
- Alternatives（否决）: 只按主知识点单算（漏涉及考点）；按主次加权（复杂，第一阶段不必）。
- Tradeoff: 各知识点关注度之和 > 真实错题数（重复计数），靠「涉及 / 共」两个口径 + UI 说明消解。

## ADR-010 · 错题时间模型（默认上传时间，可逐题改）· 2026-06-02
- Problem: 长期使用需要时间 / 来源轴做追溯，但批量录入要低摩擦。
- Constraint: 家长批量上传错题图，不想每题填时间。
- Decision: 每道错题 `occurred_at` 默认 = 批次上传时间，可逐题手动改；批次按上传时间，附可选批次名（默认日期，可改如「期中卷」）。
- Alternatives（否决）: 强制每题 / 每批填来源（增负担）；完全不记时间（断了长期趋势的根）。
- Tradeoff: 默认时间不精确（= 上传日而非做题日），但可改、不阻塞，且为后续趋势留好数据基础。

## ADR-011 · 识别兜底：手动改归类 + AI / 人工留痕 · 2026-06-02
- Problem: AI 归类不可能 100% 准，归错会让染色 / 报告失真。
- Constraint: 这是识别的兜底（非新功能维度），不算扩 MVP 范围。
- Decision: MVP 第一阶段就支持手动改某题归类（改主考点 / 增删涉及考点）。用 `mistake_kp` 的 `source`（ai / manual）+ `is_active` 留痕：旧 ai 行 is_active=0 保留，新 manual 行生效，聚合只认 is_active=1。
- Alternatives（否决）: 完全信任 AI（报告失真）；看 spike 再定（识别必有错，兜底是确定需求）。
- Tradeoff: 多一套留痕逻辑 + 改归类 UI，换报告可信 + 可审计 AI 原判 vs 人工修正。

## ADR-012 · 识别逻辑放 Rust 后端 · 2026-06-02
- Problem: 调 newapi（含令牌 + 图片 data URI）的逻辑放 Rust 后端还是前端 JS。
- Constraint: 令牌不能暴露在 WebView；批量识别是 IO 密集多请求；要复用 spike 已验证逻辑。
- Decision: 识别调用（构造 payload / Bearer / 解析 / 校验 / 落库）全在 Rust 后端，前端纯展示。令牌只在 Rust 进程内存，不进 WebView、不落明文 DB。
- Alternatives（否决）: 前端 JS 调 API（令牌暴露在 WebView / devtools、CORS / 限流难管）。
- Tradeoff: 要把 spike 的 Python 逻辑移植成 Rust，但安全 + 并发 + 出网收口最佳。

## ADR-013 · 前端用 Svelte · 2026-06-02
- Problem: Tauri 前端框架选型。
- Constraint: 包体小（合 ADR-005 体积 / 少误报）、MVP 界面不复杂、树形染色视图需响应式渲染。
- Decision: Svelte + TS + Vite（轻量、运行时小、Tauri 一等支持）。候选 React / Vue，最终按开发者熟悉度定。
- Alternatives（否决）: React（运行时偏重）；Vue（可接受，次选）。框架非风险点。
- Tradeoff: Svelte 生态组件较少，但 MVP 界面简单，可自写 / 用轻量库。
