# ARCHITECTURE — student-kpscan
<!-- 内部结构 + 不能动什么。决策理由→DECISIONS(这里只放结论+链接)；对外契约→SPEC。 -->

## 组件 + 数据流

**MVP 目标链路**（待开发，spike 通过后照下方「实现蓝图」做）：
1. 家长选错题图文件夹，Tauri app 批量导入。
2. Rust 后端逐图调 newapi vision：读印刷体题目 + 喂知识点树归类（识别输出契约见 SPEC）。
3. 落 SQLite：错题 + 错题↔知识点关联（一题点亮它涉及的多个知识点）。
4. 宏观层：错题按 `kp_id` 前缀向上聚合，染色知识树（节点显示「涉及 N 道」，顶部「共 X 道」）。
5. 微观层：点知识树节点 → 看名下错题原图，可回看。
6. 家长 / 孩子可手动改某题归类（兜底），实时影响染色。
7. 生成诊断报告（需关注知识点排序 + 可导出）。

**当前已就绪（spike，非 MVP）**：`spike/recognize.py`（check / run / score）+ `knowledge/zhejiang-math-kp-v0.yaml`。数据流：图 → base64 → newapi vision → JSON → out/。

## 关键技术选型（理由 → DECISIONS）
- 桌面 Tauri（Rust + 前端），Windows 目标 → DECISIONS-005。
- 识别逻辑全放 Rust 后端，前端纯展示 → DECISIONS-012。
- 前端 Svelte + TS + Vite → DECISIONS-013。
- 识别走 newapi 网关 vision，不直连厂商 → DECISIONS-001。
- 本地 SQLite 存储 → DECISIONS-003。
- 知识库大模型生成 v0（YAML），考纲到位再校准 → DECISIONS-002。
- 双层结构（知识树宏观 + 错题本微观）→ DECISIONS-008。

## 禁改项 / Forbidden Refactors
- **知识点树 ID 不可变**：ID 分配后只改 name 不改 ID，否则历史统计聚合断裂（聚合键 = `kp_id`）。
- **识别必经 newapi，key 只在 Rust 后端**：不在客户端直连厂商 / 不硬编码厂商 key / 令牌不进 WebView、不落明文 DB。
- **spike 未过不进 MVP**：识别率未验证通过前，不动 Tauri / SQLite / 报告等 MVP 代码（gating）。

## MVP 实现蓝图（spike 通过后照做）

### 技术栈与职责
- **Rust 后端**（业务真相源）：`config`（配置三层优先级 + endpoint 归一）、`kp_tree`（YAML 加载 + id_index + 校验，常驻内存）、`vision`（prompt 构造 / base64 / 调 newapi / JSON 解析 / id 校验）、`db`（SQLite）、`aggregate`（染色聚合）、`import`（批量编排）、`commands`（暴露前端）。识别调用全在此（key 不进 WebView）。
- **前端**（纯展示）：批量导入 + 进度、知识树染色视图、错题列表 / 详情、手动改归类、newapi 配置。公式用 KaTeX，原图走 Tauri asset 协议从本地路径加载（不入库）。
- Rust 依赖：tauri / tokio / reqwest(rustls-tls) / serde(_json / _yaml) / rusqlite / base64 / sha2 / dotenvy。
- 前后端经 Tauri command（请求 / 结果）+ event（`import_progress` 流式进度）通信。

### 复用 spike（recognize.py 逻辑移植 Rust，prompt 正文照搬保证行为一致）
config 三层优先级 + normalize_endpoint、load_kp_tree（id_index + 前缀校验 + vol_short）、build_system_prompt、encode_image、call_vision（payload / Bearer / temperature=0 / max_tokens=1500 / detail=high）、parse_model_json、recognize_one（id_in_tree 校验）。

### SQLite schema（5 表）
- `batch`：id, name（默认导入日期、可改如「期中卷」）, source_path, imported_at, status, 计数。
- `mistake`：id, batch_id, image_path, image_hash（sha256 去重）, occurred_at（默认 = imported_at、可逐题改）, read_text, has_figure, raw_response（留证）, recog_status（ok / failed / not_in_tree / pending）, recog_error, model_name。
- `mistake_kp`：mistake_id, kp_id, role（primary / alt）, source（ai / manual）, is_active（留痕，聚合只认 1）, confidence。一题多知识点 + 兜底留痕的核心。
- `kp_node`：id, parent_id, level, name, path, tree_version。YAML 同步的副本（id 不变只改 name），供外键 + 历史快照解释。
- `app_config`：GUI 可写配置。**newapi 令牌不落明文 DB**，沿用 .env / 环境变量读，次选 OS keychain。
- 关注度用**实时聚合查询**，不做物化表（数据量小、毫秒级；避免改归类后刷缓存的一致性负担）。

### 染色聚合算法（防多知识点对父节点重复累加）
错题归叶子 → 取 active 关联集合 → 每个 kp_id 按点分前缀展开祖先链（`8A.2.7.1` → `8A.2.7` → `8A.2` → `8A`）→ 每节点维护 `HashSet<mistake_id>`，同题对同祖先只 insert 一次。
- 节点「涉及 N 道」= 子树下被点亮的不同错题数（同题多点对父节点只算 1）。
- 顶部「共 X 道」= `mistake` 去重总数（口径不同于「涉及」，UI 须说清）。
- primary / alt 一视同仁计入；详情页用 `role` 区分主考点 vs 涉及考点。

### 批量导入
选文件夹 → 建 batch + mistake(pending) → tokio 信号量并发（默认 2~3）+ 有限重试（仅网络 / 超时 / 5xx；4xx 不重试）→ 落 mistake_kp(ai；alt 命中树才入，自造 id 丢弃，primary 自造则标 not_in_tree 待人工)→ `import_progress` 事件流式进度 → 不中断整批，支持「仅重试失败项」。

### 识别兜底
详情页改归类：改主考点 / 增删涉及考点 → 旧 ai 行置 is_active=0（留痕）+ 新 manual 行 is_active=1（生效）→ 实时影响染色。not_in_tree / failed 高亮引导手动归类。

### 实现顺序
Step 0 骨架 + 配置（config_check ≈ spike check）→ 1 知识树加载 + SQLite → 2 单图识别端到端（作 spike → Rust 行为回归点）→ 3 批量导入 + 落库 → 4 聚合 + 染色树 → 5 微观错题列表 / 详情 → 6 手动改归类 → 7 诊断报告 + 导出 HTML → 8 Windows 打包（禁 360）。每步可单独验收。

实现时再定（不阻塞蓝图）：前端框架最终选型（按熟悉度）、并发度 / 重试默认值、染色阈值分档（建议 0 / 1-2 / 3-5 / 6+ 四档）、令牌存储（.env vs keychain）、报告导出形式（HTML 打印 PDF vs 后端生成）。
