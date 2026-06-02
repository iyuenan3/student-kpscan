# DEPLOYMENT — student-kpscan
<!-- 跑哪/怎么跑/共享什么。key→哪都不写。共享底座属本项目就写这；消费别人的只在 RELATIONS 指属主。 -->

## 主机 + 环境
- **MVP 目标**：Windows 办公环境（**禁装 360**）。
- **当前**：无部署。仅 spike 在开发机本地跑（Python 3，建议 venv）。

## 怎么起
**spike（当前）**：
```bash
cd spike
pip install -r requirements.txt
cp .env.example .env            # 编辑 .env 填 newapi 端点 / 令牌 / vision 模型名
python recognize.py check       # 不调 API，先校验配置 + 知识点树
python recognize.py run --image 某张错题.jpg   # 单图跑通链路
python recognize.py run --dir 错题目录/         # 批量，出 out/review.csv
python recognize.py score --review out/review.csv   # 人工填表后算准确率
```

**MVP（待开发）**：Tauri build → Windows 安装包。⚑ 打包 / 代码签名 / 减少杀软误报流程待定（选 Tauri 的部分原因即体积小、相对少触发杀软）。

## 域名 / 入口
N/A — 本地桌面 app，无域名 / 无对外服务入口。

## 共享底座引用
识别依赖私有 newapi 网关（OpenAI 兼容）。本项目只在 `spike/.env` 配端点 + 令牌 + 模型名（不入库），不含网关本身的部署 / 证书配置。

## 备份 / 升级 / 回滚
⚑ 待 MVP。本地 SQLite 数据（错题 / 识别结果 / 统计）备份策略待定；学生试卷数据本地优先、不留云。

## 运维约束
- Windows **禁装 360**（减少杀软误报，选 Tauri 体积小的部分原因）。
- 客户端**不能 ship 主 key**（分发时 key 管理待解，见 ROADMAP / 风险）。
- 学生试卷图含未成年人数据：不留云、不外泄、规模化前评估隐私（见 CORE 红线）。
