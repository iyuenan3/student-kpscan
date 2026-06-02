# CLAUDE.md：student-kpscan

> 初中错题 AI 诊断辅导工具（纯工具型，浙江中考 / 数学 MVP）。
> 本文件 = router（状态 + 路由 + 红线 + 维护责任 + 命令）。实质内容在 `AIREADME/`（AI 真相源，入口 `AIREADME/INDEX.md`）。

## 当前状态（2026-06-01 立项）

- 阶段：第 0 步识别率 spike 工具就绪并自测通过（知识点树 v0 + spike 脚本），待外部输入后正式跑 spike，未进入 MVP 开发。
- 下一步：newapi 网关侧接入 vision chat 渠道（见 `AIREADME/RELATIONS.md`）+ 拿需求方真实错题样本，跑 spike 测读题 / 归类准确率，通过才进 MVP。卡的外部输入详见 `AIREADME/ROADMAP.md`、`spike/README.md`。

## 加载路由（按任务读 AIREADME）

- 了解项目 / 边界 / 红线 → `CORE.md`
- 生态依赖（newapi 网关）→ `RELATIONS.md`
- 识别 / 知识点契约 → `SPEC.md`
- 架构 / 选型 / 禁改 → `ARCHITECTURE.md` + `DECISIONS.md`
- 产品意图 / 商业 / 风险 → `PRD.md`
- 节奏 / 学科优先级 → `ROADMAP.md`
- 编码 / 写作约定 → `CONVENTIONS.md`
- 部署 → `DEPLOYMENT.md`
- 跑 spike → `spike/README.md`

## 红线（完整见 `AIREADME/CORE.md`「绝不」）

- 不提交任何 key / 密钥进 git。
- 识别一律走 newapi 网关 vision，不直连厂商。
- 学生试卷图（未成年人数据）：不外泄 / 不留云 / 样本不入库。
- spike 是硬 gating：未验证通过不开始 Tauri / MVP。
- MVP 范围不扩：不判对错 / 红叉 / 推荐 / 其他学科 / 移动端。
- 中文全角标点，绝不用破折号（—— / —）。

## 维护责任（什么变更新哪个）

- 定位 / 红线 → CORE；重大决策 → DECISIONS（append）；架构 / 选型 → ARCHITECTURE；契约 → SPEC；部署 → DEPLOYMENT；优先级 → ROADMAP；事故 → MEMORY（append）；里程碑 → CHANGELOG（append）；任何文件状态变 → INDEX 状态表 + last-synced。
- 校验：`bash ~/.claude/skills/aireadme/check.sh AIREADME`

## 常用命令

spike（第 0 步识别率验证，gating，详见 `spike/README.md`）：

```bash
cd spike && pip install -r requirements.txt && cp .env.example .env  # 首次：填 newapi 端点 / 令牌 / vision 模型名
python recognize.py check                          # 不调 API，校验配置 + 知识点树
python recognize.py run --image 某张错题.jpg        # 单图跑通链路
python recognize.py run --dir 错题目录/             # 批量，出 out/review.csv 待人工填
python recognize.py score --review out/review.csv  # 人工填表后算准确率
```

（Tauri dev / build / Windows 打包待 spike 通过、进入 MVP 后补。）
