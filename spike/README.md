# 第 0 步：识别率 spike

整条产品链路唯一的硬不确定性 = 数学题多模态识别 + 知识点归类。这个 spike 在不碰 Tauri / MVP 的前提下验证它能不能跑通、准不准。**spike 是 gating：没验证通过，不开始 MVP 开发。**

测两个数：

- **读题准确率**：印刷体题目文本（含公式 / 几何图）转写得对不对。
- **归类准确率**：归到的知识点 id 对不对。

两个数都需要人工 ground truth（老师 / 家长核对，见 PRD「知识点归类无 ground truth」），所以脚本分两步：`run` 让模型产出结果并生成评分表，`score` 读人工填好的评分表算准确率。

## 一次性准备

```bash
cd spike
pip install -r requirements.txt
cp .env.example .env          # 然后编辑 .env，填 newapi 端点 / 令牌 / vision 模型名
```

`.env` 已被 `.gitignore` 忽略，令牌不会进 git。

## 用法

```bash
# 1. 不调 API，先校验配置 + 知识点树（建议第一步先跑这个）
python recognize.py check
python recognize.py check --show-prompt      # 顺便看喂给模型的 prompt 长啥样

# 2. 单图：先跑通 newapi 链路（确认端点能读图 + 返回 JSON）
python recognize.py run --image /路径/某张错题.jpg

# 3. 批量：识别整个目录，写出 out/results.jsonl + out/review.csv
python recognize.py run --dir /路径/错题目录 --limit 5     # --limit 先试手
python recognize.py run --dir /路径/错题目录

# 4. 人工评分：打开 out/review.csv，逐行填两列
#    「读题正确(1=对/0.5=部分/0=错)」「归类正确(1/0)」，归类错时可在「正确知识点ID」填对的 id
#    存盘后算准确率：
python recognize.py score --review out/review.csv
```

## 输出物

- `out/results.jsonl`：每张图一条原始记录（模型原文、解析结果、耗时、tokens）。
- `out/review.csv`：人工评分表（UTF-8 BOM，Excel 直接打开不乱码），前几列是模型结果，后几列留空待人工填。

> `out/`、`.env`、错题样本都不入库（`.gitignore` 已挡 `samples/`、`*.env` 等；评分表 / 结果含娃试卷转写文本，也不要提交）。

## 设计说明

- 识别走 newapi 的 OpenAI 兼容端点（`/v1/chat/completions`），图片以 base64 data URI 传，`detail=high` 便于读小字和公式。不直连厂商。
- 归类时把整棵知识点树展平喂给模型，要求它只能从给定 id 里选，不得自造；脚本会标记「自造 id」。
- `temperature=0` 求稳定可复现。报错时原样打印 newapi 返回体（渠道没配 / 模型名错 / 余额不足等都看得见）。

## 跑正式 spike 还缺什么

见仓库根 `README` 与本次交接说明：需要 newapi 配好 vision 渠道（拿到端点 + 令牌 + 模型名），以及需求方提供的真实错题样本图。
