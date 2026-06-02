# ARCHITECTURE — student-kpscan
<!-- 内部结构 + 不能动什么。决策理由→DECISIONS(这里只放结论+链接)；对外契约→SPEC。 -->

## 组件 + 数据流

**MVP 目标链路**（待开发）：
1. 家长把错题照片放进一个文件夹。
2. Tauri 桌面 app 批量导入该文件夹。
3. 逐图调 newapi vision：读印刷体题目文本 + 喂知识点树做归类（识别输出契约见 SPEC）。
4. 聚合：按 `kp_id` 统计错题数，不区分错误类型（错了即该知识点薄弱）。
5. 生成诊断报告：知识点薄弱点排序 + 错题数 + 可导出（PDF / HTML）。
6. 本地 SQLite 存：错题、识别结果、知识点统计。

**当前已就绪（spike，非 MVP）**：
- `spike/recognize.py`（Python，纯调 API、不碰 Tauri）：三命令 `check`（不调 API 校验配置 + 知识点树）/ `run`（`--image` 单图、`--dir` 批量）/ `score`（人工填 `review.csv` 后算读题 / 归类准确率）。
- `knowledge/zhejiang-math-kp-v0.yaml`：知识点树（归类 schema）。
- 数据流：图 → base64 → newapi vision → JSON → `out/results.jsonl` + `out/review.csv`（待人工评分）。

## 关键技术选型（理由 → DECISIONS）
- 桌面 Tauri（Rust + 前端），Windows 目标 → DECISIONS-005。
- 识别走 newapi 网关 vision，不直连厂商 → DECISIONS-001。
- 本地 SQLite 存储 → DECISIONS-003（随 MVP 范围一并定）。
- 知识点库大模型生成 v0（YAML），考纲到位再校准 → DECISIONS-002。

## 禁改项 / Forbidden Refactors
- **知识点树 ID 不可变**：ID 分配后只改 name 不改 ID，否则历史统计聚合断裂（聚合键 = `kp_id`）。
- **识别必经 newapi**：不可在客户端直连厂商 / 硬编码厂商 key。
- **spike 未过不进 MVP**：识别率未验证通过前，不动 Tauri / SQLite / 报告等 MVP 代码（gating）。

⚑ 实现细节待 MVP 开发（spike 通过后填）：Tauri 模块划分、SQLite 表结构、报告模板、批量导入并发 / 重试策略。
