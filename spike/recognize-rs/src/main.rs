//! student-kpscan 识别率 spike 的 Rust 版（recognize-rs）。
//! 对照 spike/recognize.py 逐步移植：check / run / score。
//! 第五步：run 支持 --image（单图）/ --dir（批量出 review.csv），识别核心抽成 recognize_one。
#![allow(dead_code)] // KpNode.name、PointRef.name 暂未读，保留语义完整

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write; // stdout().flush()
use std::path::Path;
use std::process;
use std::time::Instant;

use base64::prelude::*;
use serde::Deserialize;

// ===== LLM 配置 =====
struct Config {
    base_url: String,
    key: String,
    model: String,
    ca_cert: String,
}

// ===== 知识点树结构 =====
#[derive(Deserialize)]
struct KpTree {
    meta: Meta,
    volumes: Vec<Volume>,
}
#[derive(Deserialize)]
struct Meta {
    version: String,
    status: String,
}
#[derive(Deserialize)]
struct Volume {
    id: String,
    grade: String,
    volume: String,
    chapters: Vec<Chapter>,
}
#[derive(Deserialize)]
struct Chapter {
    id: String,
    name: String,
    sections: Vec<Section>,
}
#[derive(Deserialize)]
struct Section {
    id: String,
    name: String,
    points: Vec<Point>,
}
#[derive(Deserialize)]
struct Point {
    id: String,
    name: String,
}

// ===== 树加载产物 =====
struct KpNode {
    name: String,
    path: String,
    level: String,
}
struct PointRef {
    id: String,
    name: String,
    path: String,
}
struct LoadedTree {
    id_index: HashMap<String, KpNode>,
    points: Vec<PointRef>,
    issues: Vec<String>,
}

// ===== 单图识别结果（recognize_one 的返回；单图打印、批量落表都用它）=====
struct RecogResult {
    image: String,
    ok: bool,
    error: String,
    raw: String,
    parsed: Option<serde_json::Value>,
    primary_kp_id: String,
    primary_kp_path: String,
    in_tree: bool,
    latency_sec: f64,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

const SYSTEM_PROMPT_HEAD: &str = r#"你是浙江初中数学错题识别助手。我会给你一张错题图片（机器印刷体题目，可能含数学公式和几何图 / 函数图象）。请完成两件事，并且只输出一个 JSON 对象，不要写任何解释、不要用 markdown 代码块。

任务一，读题：转写图片中的印刷体数学题目文本。公式用 LaTeX（行内写成 $...$）。忽略手写笔迹、红叉、老师批改等非题干内容。若图中有多道题，转写最完整的一道。

任务二，归类：从下面给定的知识点清单中，选出该题最匹配的 1 个知识点 id 作为 primary_kp_id，可再给最多 2 个备选放进 alt_kp_ids。只能选清单中真实存在的 id，绝不要自造 id。尽量归到最细的「点」级 id；若把握不大，可以只给到「节」级（去掉 id 最后一段，如 8A.2.7）或「章」级（如 8A.2），并相应调低 confidence。

输出 JSON，字段如下：
{
  "read_text": "转写的题目文本，公式用 LaTeX",
  "has_figure": true 或 false,
  "primary_kp_id": "如 8A.2.7.1",
  "primary_kp_name": "该 id 对应的知识点名称",
  "alt_kp_ids": ["备选id", "..."],
  "confidence": 0 到 1 之间的小数,
  "reason": "一句话归类依据"
}

知识点清单（格式为：id | 册 > 章 > 节 > 点）：
"#;

const REVIEW_HEADERS: [&str; 12] = [
    "图片",
    "模型读题文本",
    "含图形",
    "归类ID(主)",
    "归类路径(主)",
    "备选ID",
    "置信度",
    "模型理由",
    "读题正确(1=对/0.5=部分/0=错)",
    "归类正确(1/0)",
    "正确知识点ID(可选填)",
    "备注",
];

fn vol_short(grade: &str, volume: &str) -> String {
    let g = match grade {
        "七年级" => "七",
        "八年级" => "八",
        "九年级" => "九",
        other => other,
    };
    let v = match volume {
        "上册" => "上",
        "下册" => "下",
        other => other,
    };
    format!("{g}{v}")
}

fn is_child(child_id: &str, parent_id: &str) -> bool {
    child_id.starts_with(parent_id) && child_id[parent_id.len()..].starts_with('.')
}

fn register(
    idx: &mut HashMap<String, KpNode>,
    issues: &mut Vec<String>,
    id: &str,
    name: &str,
    path: &str,
    level: &str,
) {
    if idx.contains_key(id) {
        issues.push(format!("重复 id：{id}"));
    }
    idx.insert(
        id.to_string(),
        KpNode {
            name: name.to_string(),
            path: path.to_string(),
            level: level.to_string(),
        },
    );
}

fn load_config() -> Result<Config, String> {
    let env_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../.env");
    let content = fs::read_to_string(env_path)
        .map_err(|e| format!("读不到 {env_path}：{e}（先 cp .env.example .env 并填好）"))?;

    let mut base_url = String::new();
    let mut key = String::new();
    let mut model = String::new();
    let mut ca_cert = String::new();

    for raw in content.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            match k.trim() {
                "NEWAPI_BASE_URL" => base_url = v.to_string(),
                "NEWAPI_KEY" => key = v.to_string(),
                "NEWAPI_MODEL" => model = v.to_string(),
                "NEWAPI_CA_CERT" => ca_cert = v.to_string(),
                _ => {}
            }
        }
    }

    if base_url.is_empty() || key.is_empty() || model.is_empty() {
        return Err("NEWAPI_BASE_URL / NEWAPI_KEY / NEWAPI_MODEL 三项缺一不可".to_string());
    }
    Ok(Config { base_url, key, model, ca_cert })
}

fn load_kp_tree() -> Result<KpTree, String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../knowledge/zhejiang-math-kp-v0.yaml");
    let text = fs::read_to_string(path).map_err(|e| format!("读不到知识点树 {path}：{e}"))?;
    serde_yaml::from_str(&text).map_err(|e| format!("知识点树 YAML 解析失败：{e}"))
}

fn build_index(tree: &KpTree) -> LoadedTree {
    let mut id_index: HashMap<String, KpNode> = HashMap::new();
    let mut points: Vec<PointRef> = Vec::new();
    let mut issues: Vec<String> = Vec::new();

    for vol in &tree.volumes {
        let vshort = vol_short(&vol.grade, &vol.volume);
        register(&mut id_index, &mut issues, &vol.id, &vshort, &vshort, "册");
        for chap in &vol.chapters {
            let cpath = format!("{vshort} > {}", chap.name);
            register(&mut id_index, &mut issues, &chap.id, &chap.name, &cpath, "章");
            if !is_child(&chap.id, &vol.id) {
                issues.push(format!("章 id 前缀不符：{} 不在册 {} 下", chap.id, vol.id));
            }
            for sec in &chap.sections {
                let spath = format!("{cpath} > {}", sec.name);
                register(&mut id_index, &mut issues, &sec.id, &sec.name, &spath, "节");
                if !is_child(&sec.id, &chap.id) {
                    issues.push(format!("节 id 前缀不符：{} 不在章 {} 下", sec.id, chap.id));
                }
                for pt in &sec.points {
                    let ppath = format!("{spath} > {}", pt.name);
                    register(&mut id_index, &mut issues, &pt.id, &pt.name, &ppath, "点");
                    if !is_child(&pt.id, &sec.id) {
                        issues.push(format!("点 id 前缀不符：{} 不在节 {} 下", pt.id, sec.id));
                    }
                    points.push(PointRef {
                        id: pt.id.clone(),
                        name: pt.name.clone(),
                        path: ppath,
                    });
                }
            }
        }
    }
    LoadedTree { id_index, points, issues }
}

fn normalize_endpoint(base: &str) -> String {
    let url = base.trim_end_matches('/');
    if url.ends_with("/chat/completions") {
        url.to_string()
    } else if url.ends_with("/v1") {
        format!("{url}/chat/completions")
    } else {
        format!("{url}/v1/chat/completions")
    }
}

fn build_system_prompt(points: &[PointRef]) -> String {
    let kp_block: String = points
        .iter()
        .map(|p| format!("{} | {}", p.id, p.path))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}{}", SYSTEM_PROMPT_HEAD, kp_block)
}

fn encode_image(path: &str) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("读不到图 {path}：{e}"))?;
    let mime = match path.rsplit('.').next().map(|s| s.to_lowercase()).as_deref() {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    };
    let b64 = BASE64_STANDARD.encode(&bytes);
    Ok(format!("data:{mime};base64,{b64}"))
}

fn build_client(cfg: &Config) -> Result<reqwest::blocking::Client, String> {
    let mut builder = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(120));
    if !cfg.ca_cert.is_empty() {
        let pem = fs::read(&cfg.ca_cert).map_err(|e| format!("读不到 CA 证书 {}：{e}", cfg.ca_cert))?;
        let cert = reqwest::Certificate::from_pem(&pem).map_err(|e| format!("CA 证书解析失败：{e}"))?;
        builder = builder.add_root_certificate(cert);
    }
    builder.build().map_err(|e| format!("构建 HTTP 客户端失败：{e}"))
}

/// 调 vision 端点，返回 (模型输出文本, usage 对象)。
fn call_vision(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    key: &str,
    model: &str,
    system_prompt: &str,
    data_uri: &str,
) -> Result<(String, serde_json::Value), String> {
    let payload = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": [
                { "type": "text", "text": "请识别这张错题图，并按要求只输出 JSON。" },
                { "type": "image_url", "image_url": { "url": data_uri, "detail": "high" } }
            ]}
        ],
        "temperature": 0,
        "max_tokens": 1500
    });

    let resp = client
        .post(endpoint)
        .header("Authorization", format!("Bearer {key}"))
        .json(&payload)
        .send()
        .map_err(|e| format!("请求失败：{e}"))?;

    let status = resp.status();
    let body = resp.text().map_err(|e| format!("读响应失败：{e}"))?;
    if !status.is_success() {
        let snippet: String = body.chars().take(800).collect();
        return Err(format!("HTTP {status}：{snippet}"));
    }

    let data: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("响应非 JSON：{e}"))?;
    match data["choices"][0]["message"]["content"].as_str() {
        Some(c) => Ok((c.to_string(), data["usage"].clone())),
        None => {
            let snippet: String = body.chars().take(300).collect();
            Err(format!("响应里没有 choices[0].message.content：{snippet}"))
        }
    }
}

fn parse_model_json(text: &str) -> Result<serde_json::Value, String> {
    let start = text.find('{').ok_or("输出里找不到 {")?;
    let end = text.rfind('}').ok_or("输出里找不到 }")?;
    if end <= start {
        return Err("JSON 边界异常".to_string());
    }
    serde_json::from_str(&text[start..=end]).map_err(|e| format!("JSON 解析失败：{e}"))
}

/// 单图识别：编码 → 调用 → 解析 → id 校验，全部装进 RecogResult。失败也不 panic，记进 error。
fn recognize_one(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    key: &str,
    model: &str,
    system_prompt: &str,
    id_index: &HashMap<String, KpNode>,
    image: &str,
) -> RecogResult {
    let mut r = RecogResult {
        image: image.to_string(),
        ok: false,
        error: String::new(),
        raw: String::new(),
        parsed: None,
        primary_kp_id: String::new(),
        primary_kp_path: String::new(),
        in_tree: false,
        latency_sec: 0.0,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
    };

    let data_uri = match encode_image(image) {
        Ok(u) => u,
        Err(e) => {
            r.error = e;
            return r;
        }
    };

    let t0 = Instant::now();
    let (raw, usage) = match call_vision(client, endpoint, key, model, system_prompt, &data_uri) {
        Ok(x) => x,
        Err(e) => {
            r.error = e;
            return r;
        }
    };
    r.latency_sec = t0.elapsed().as_secs_f64();
    r.raw = raw.clone();
    r.prompt_tokens = usage["prompt_tokens"].as_u64().unwrap_or(0);
    r.completion_tokens = usage["completion_tokens"].as_u64().unwrap_or(0);
    r.total_tokens = usage["total_tokens"].as_u64().unwrap_or(0);

    match parse_model_json(&raw) {
        Ok(p) => {
            let pid = p["primary_kp_id"].as_str().unwrap_or("").to_string();
            r.in_tree = id_index.contains_key(&pid);
            r.primary_kp_path = id_index.get(&pid).map(|n| n.path.clone()).unwrap_or_default();
            r.primary_kp_id = pid;
            r.parsed = Some(p);
            r.ok = true;
        }
        Err(e) => r.error = e,
    }
    r
}

fn print_single(r: &RecogResult) {
    println!("\n==== 识别结果：{} ====", r.image);
    if !r.ok {
        println!("失败：{}", r.error);
        return;
    }
    let p = r.parsed.as_ref().unwrap();
    println!("读题文本：\n{}", p["read_text"].as_str().unwrap_or(""));
    println!("\n含图形：{}", p["has_figure"]);
    let path_show = if r.primary_kp_path.is_empty() {
        "(不在知识点树中！)"
    } else {
        r.primary_kp_path.as_str()
    };
    println!("归类（主）：{}  {}", r.primary_kp_id, path_show);
    println!("备选：{}", p["alt_kp_ids"]);
    println!("置信度：{}", p["confidence"]);
    println!("依据：{}", p["reason"].as_str().unwrap_or(""));
    if !r.in_tree {
        println!("⚠ primary_kp_id 不在知识点树里，可能自造，归类不可信。");
    }
    println!(
        "\n耗时 {:.2}s，tokens 提示 {} / 生成 {}",
        r.latency_sec, r.prompt_tokens, r.completion_tokens
    );
}

fn gather_images(dir: &str) -> Result<Vec<String>, String> {
    let rd = fs::read_dir(dir).map_err(|e| format!("读不到目录 {dir}：{e}"))?;
    let exts = ["jpg", "jpeg", "png", "webp", "bmp", "gif"];
    let mut imgs: Vec<String> = Vec::new();
    for ent in rd {
        let path = ent.map_err(|e| format!("遍历出错：{e}"))?.path();
        let ext_l = match path.extension().and_then(|s| s.to_str()) {
            Some(e) => e.to_lowercase(),
            None => continue,
        };
        if exts.contains(&ext_l.as_str()) {
            if let Some(s) = path.to_str() {
                imgs.push(s.to_string());
            }
        }
    }
    imgs.sort();
    if imgs.is_empty() {
        return Err(format!("目录里没有图片（支持 {}）：{dir}", exts.join("/")));
    }
    Ok(imgs)
}

fn write_review_csv(results: &[RecogResult], path: &Path) -> Result<(), String> {
    let mut wtr = csv::Writer::from_path(path).map_err(|e| format!("建 review.csv 失败：{e}"))?;
    wtr.write_record(REVIEW_HEADERS).map_err(|e| format!("写表头失败：{e}"))?;
    for r in results {
        let row: Vec<String> = if r.ok {
            let p = r.parsed.as_ref().unwrap();
            let alt = p["alt_kp_ids"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            let pid_mark = if r.in_tree {
                r.primary_kp_id.clone()
            } else {
                format!("{} (不在树中)", r.primary_kp_id)
            };
            vec![
                r.image.clone(),
                p["read_text"].as_str().unwrap_or("").to_string(),
                p["has_figure"].to_string(),
                pid_mark,
                r.primary_kp_path.clone(),
                alt,
                p["confidence"].to_string(),
                p["reason"].as_str().unwrap_or("").to_string(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ]
        } else {
            let mut v = vec![r.image.clone(), format!("[识别失败] {}", r.error)];
            v.resize(12, String::new());
            v
        };
        wtr.write_record(&row).map_err(|e| format!("写行失败：{e}"))?;
    }
    wtr.flush().map_err(|e| format!("flush 失败：{e}"))?;
    Ok(())
}

fn cmd_check() -> Result<(), String> {
    let cfg = load_config()?;
    println!("== 配置检查 ==");
    println!("  NEWAPI_BASE_URL：{}", cfg.base_url);
    println!("  NEWAPI_KEY：已设置（{} 位，已隐藏）", cfg.key.len());
    println!("  NEWAPI_MODEL：{}", cfg.model);
    println!(
        "  NEWAPI_CA_CERT：{}",
        if cfg.ca_cert.is_empty() { "(未设置，用系统 CA)" } else { cfg.ca_cert.as_str() }
    );

    let tree = load_kp_tree()?;
    let loaded = build_index(&tree);
    let count = |lvl: &str| loaded.id_index.values().filter(|n| n.level == lvl).count();

    println!("\n== 知识点树检查 ==");
    println!(
        "  册 {} / 章 {} / 节 {} / 点 {}（叶子 {} 个）",
        count("册"),
        count("章"),
        count("节"),
        count("点"),
        loaded.points.len()
    );
    println!("  meta.version={} status={}", tree.meta.version, tree.meta.status);
    if loaded.issues.is_empty() {
        println!("  结构自洽：id 唯一、层级前缀一致。");
    } else {
        println!("  结构问题 {} 处：", loaded.issues.len());
        for s in &loaded.issues {
            println!("    - {s}");
        }
    }
    Ok(())
}

/// 准备识别上下文（配置 / 树 / prompt / client），单图和批量都要。
struct RunCtx {
    cfg: Config,
    loaded: LoadedTree,
    endpoint: String,
    system_prompt: String,
    client: reqwest::blocking::Client,
}

fn prepare_run() -> Result<RunCtx, String> {
    let cfg = load_config()?;
    let tree = load_kp_tree()?;
    let loaded = build_index(&tree);
    let endpoint = normalize_endpoint(&cfg.base_url);
    let system_prompt = build_system_prompt(&loaded.points);
    let client = build_client(&cfg)?;
    Ok(RunCtx { cfg, loaded, endpoint, system_prompt, client })
}

fn cmd_run_image(image: &str) -> Result<(), String> {
    let ctx = prepare_run()?;
    println!(
        "端点 {} | 模型 {} | 知识点 {} 个 | 图 {image}",
        ctx.endpoint,
        ctx.cfg.model,
        ctx.loaded.points.len()
    );
    let r = recognize_one(
        &ctx.client,
        &ctx.endpoint,
        &ctx.cfg.key,
        &ctx.cfg.model,
        &ctx.system_prompt,
        &ctx.loaded.id_index,
        image,
    );
    print_single(&r);
    Ok(())
}

fn cmd_run_dir(dir: &str) -> Result<(), String> {
    let ctx = prepare_run()?;
    let images = gather_images(dir)?;
    println!(
        "端点 {} | 模型 {} | 知识点 {} 个 | 待识别 {} 张",
        ctx.endpoint,
        ctx.cfg.model,
        ctx.loaded.points.len(),
        images.len()
    );

    let mut results: Vec<RecogResult> = Vec::new();
    let t_start = Instant::now();
    for (i, img) in images.iter().enumerate() {
        print!("[{}/{}] {} ... ", i + 1, images.len(), img);
        std::io::stdout().flush().ok();
        let r = recognize_one(
            &ctx.client,
            &ctx.endpoint,
            &ctx.cfg.key,
            &ctx.cfg.model,
            &ctx.system_prompt,
            &ctx.loaded.id_index,
            img,
        );
        if r.ok {
            let conf = r.parsed.as_ref().map(|p| p["confidence"].to_string()).unwrap_or_default();
            println!("ok {:.1}s -> {}（置信度 {}）", r.latency_sec, r.primary_kp_id, conf);
        } else {
            println!("失败：{}", r.error.chars().take(80).collect::<String>());
        }
        results.push(r);
    }

    // 输出目录 spike/out（.gitignore 已挡，识别产物不入库）
    let out_dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../out"));
    fs::create_dir_all(out_dir).map_err(|e| format!("建输出目录失败：{e}"))?;

    let mut jsonl = String::new();
    for r in &results {
        let line = serde_json::json!({
            "image": r.image, "ok": r.ok, "error": r.error,
            "primary_kp_id": r.primary_kp_id, "in_tree": r.in_tree,
            "latency_sec": r.latency_sec,
            "prompt_tokens": r.prompt_tokens,
            "completion_tokens": r.completion_tokens,
            "total_tokens": r.total_tokens,
            "raw": r.raw, "parsed": r.parsed,
        });
        jsonl.push_str(&line.to_string());
        jsonl.push('\n');
    }
    let jsonl_path = out_dir.join("results.jsonl");
    fs::write(&jsonl_path, &jsonl).map_err(|e| format!("写 results.jsonl 失败：{e}"))?;
    let review_path = out_dir.join("review.csv");
    write_review_csv(&results, &review_path)?;

    let ok_n = results.iter().filter(|r| r.ok).count();
    let fail_n = results.len() - ok_n;
    let not_in_tree = results.iter().filter(|r| r.ok && !r.in_tree).count();
    let has_fig = results
        .iter()
        .filter(|r| r.ok && r.parsed.as_ref().is_some_and(|p| p["has_figure"].as_bool().unwrap_or(false)))
        .count();
    let total_tok: u64 = results.iter().map(|r| r.total_tokens).sum();
    let avg_lat = if ok_n > 0 {
        results.iter().filter(|r| r.ok).map(|r| r.latency_sec).sum::<f64>() / ok_n as f64
    } else {
        0.0
    };

    println!("\n== 批量完成（耗时 {:.0}s）==", t_start.elapsed().as_secs_f64());
    println!("  成功 {ok_n} / 失败 {fail_n}");
    println!("  含图形题 {has_fig}（公式 / 几何图是识别难点，重点核这些）");
    println!("  自造 id（不在树中）{not_in_tree}");
    println!("  平均耗时 {avg_lat:.2}s / 图，合计 tokens {total_tok}");
    println!("  原始结果：{}", jsonl_path.display());
    println!("  人工评分表：{}", review_path.display());
    println!("\n下一步：打开 review.csv 逐行填「读题正确」「归类正确」两列，存盘后执行 score --review <review.csv>");
    Ok(())
}

/// 把一格文本解析成分数；空白 / 非数字返回 None（= 未评，不计入）。
fn parse_f(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        s.parse::<f64>().ok()
    }
}

/// score：读人工填好的 review.csv，算读题准确率 + 归类准确率 + 置信度校准。
fn cmd_score(review: &str) -> Result<(), String> {
    let mut rdr = csv::Reader::from_path(review).map_err(|e| format!("读不到 {review}：{e}"))?;
    // 按表头名找列号（不写死列序，容忍人工调列）。
    let headers = rdr.headers().map_err(|e| format!("读表头失败：{e}"))?.clone();
    let col = |name: &str| headers.iter().position(|h| h == name);
    let read_i = col("读题正确(1=对/0.5=部分/0=错)");
    let cls_i = col("归类正确(1/0)");
    let conf_i = col("置信度");

    let mut read_scores: Vec<f64> = Vec::new();
    let mut cls_scores: Vec<f64> = Vec::new();
    let mut conf_right: Vec<f64> = Vec::new();
    let mut conf_wrong: Vec<f64> = Vec::new();
    let mut total = 0usize;
    let mut ungraded = 0usize;

    for rec in rdr.records() {
        let rec = rec.map_err(|e| format!("读行失败：{e}"))?;
        total += 1;
        let cell = |i: Option<usize>| i.and_then(|i| rec.get(i)).unwrap_or("");
        let rv = parse_f(cell(read_i));
        let cv = parse_f(cell(cls_i));
        if let Some(r) = rv {
            read_scores.push(r);
        }
        if let Some(c) = cv {
            cls_scores.push(c);
            if let Some(conf) = parse_f(cell(conf_i)) {
                if c >= 1.0 {
                    conf_right.push(conf);
                } else {
                    conf_wrong.push(conf);
                }
            }
        }
        if rv.is_none() && cv.is_none() {
            ungraded += 1;
        }
    }

    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    println!("== 识别率 spike 评分：{review} ==");
    println!("  图片总数 {total}");
    if read_scores.is_empty() {
        println!("  读题准确率：无数据（请先填「读题正确」列）");
    } else {
        println!("  读题准确率：{:.1}%（已评 {} 张，0.5 计部分正确）", mean(&read_scores) * 100.0, read_scores.len());
    }
    if cls_scores.is_empty() {
        println!("  归类准确率：无数据（请先填「归类正确」列）");
    } else {
        println!("  归类准确率：{:.1}%（已评 {} 张）", mean(&cls_scores) * 100.0, cls_scores.len());
    }
    if !conf_right.is_empty() || !conf_wrong.is_empty() {
        let show = |v: &[f64]| if v.is_empty() { "无".to_string() } else { format!("{:.2}", mean(v)) };
        println!("  置信度校准参考：归类对时均置信度 {}，归类错时 {}", show(&conf_right), show(&conf_wrong));
    }
    if ungraded > 0 {
        println!("  还有 {ungraded} 张两列都没填，未计入。");
    }
    println!("\n  通过线由 spike 结果与需求方拍定后再判定是否进入 MVP 开发。");
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");

    let result = match cmd {
        "check" => cmd_check(),
        "run" => match args.get(2).map(String::as_str) {
            Some("--image") => match args.get(3) {
                Some(p) => cmd_run_image(p),
                None => Err("用法：run --image <图片路径>".to_string()),
            },
            Some("--dir") => match args.get(3) {
                Some(d) => cmd_run_dir(d),
                None => Err("用法：run --dir <目录>".to_string()),
            },
            _ => Err("用法：run --image <图> 或 run --dir <目录>".to_string()),
        },
        "score" => match (args.get(2).map(String::as_str), args.get(3)) {
            (Some("--review"), Some(p)) => cmd_score(p),
            _ => Err("用法：score --review <review.csv>".to_string()),
        },
        "" => Err("用法：recognize-rs <check | run --image 图 | run --dir 目录 | score>".to_string()),
        other => Err(format!("未知子命令：{other}")),
    };

    if let Err(e) = result {
        eprintln!("错误：{e}");
        process::exit(1);
    }
}
