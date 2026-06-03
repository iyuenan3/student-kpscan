# student-kpscan

初中错题 AI 诊断辅导工具（纯工具型）。拍错题 → 识别 + 关联浙江中考知识点 → 统计知识点薄弱点 → 诊断报告。

> 让孩子没有一道题是白刷的。

## 状态

立项（2026-06-01）。当前阶段：识别率 spike 工具就绪，待 newapi vision 渠道 + 真实错题样本到位后正式跑 spike，验证通过才进入 MVP 开发。

## MVP（数学 / Windows / 1 周目标）

- 批量导入错题照片（文件夹）
- 调 newapi vision 模型读印刷体数学题 + 关联中考知识点
- 生成知识点薄弱点诊断报告（可导出）

差异化：区别于猿辅导 / 作业帮以卖课为目的，本产品只做错题诊断 + 知识点薄弱定位，解决机械抄错题、盲目刷题。

## 技术栈

Tauri（Rust + React，Windows 桌面）+ newapi（OpenAI 兼容）vision 模型 + 本地 SQLite。

## 目录

- `knowledge/` 浙江初中数学知识点树（核心资产，v0 待考纲校准）
- `spike/` 第 0 步识别率 spike 脚本（gating，通过才开发 MVP）
