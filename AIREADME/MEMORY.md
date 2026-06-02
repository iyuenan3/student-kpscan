# MEMORY — student-kpscan
<!-- 踩坑/失败/事故，append-only。别重复踩坑。决策→DECISIONS。 -->

<!-- 模板：
## <现象> · YYYY-MM-DD
- 现象:
- 根因:
- 结论/避免:
-->

## .gitignore `.env.*` 误伤 `.env.example` · 2026-06-01
- 现象: 要随仓库提交的模板 `spike/.env.example` 被 git 忽略，加不进版本库。
- 根因: `.gitignore` 里 `.env.*` 通配把 `.env.example` 一并挡了（本意只挡真实 `.env`）。
- 结论/避免: 加豁免 `!.env.example`；改完用 `git check-ignore <文件>` 验证（`.env.example` 应纳入、`.env` / `out/` 应忽略）。
