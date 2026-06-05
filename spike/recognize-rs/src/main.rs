//! student-kpscan 识别率 spike 的 Rust 版（recognize-rs）。
//! 对照 spike/recognize.py，并升级为「一图多题」（真实错题本一页多题，见 DECISIONS-019）。
//! 命令：check / run --image / run --dir / score。
#![allow(dead_code)] // KpNode.name、PointRef.name 暂未读，保留语义完整

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Write;
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

// ===== 识别结果：一张图 N 道题 =====
/// 一道题的识别结果。
struct Question {
    label: String, // 图中题号（如 4 / 16 / 拓展延伸），可空
    read_text: String,
    has_figure: bool,
    primary_kp_id: String,
    primary_kp_path: String,
    in_tree: bool,
    alt_kp_ids: Vec<String>,
    confidence: f64,
    reason: String,
}
/// 一张图的识别结果（含它上面的所有题）。
struct ImageResult {
    image: String,
    ok: bool,
    error: String,
    raw: String,
    latency_sec: f64,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    questions: Vec<Question>,
}

// ===== system prompt：一图多题，输出 JSON 数组 =====
const SYSTEM_PROMPT_HEAD: &str = r#"你是浙江初中数学错题识别助手。我会给你一张图，图中通常有多道数学题（错题本常一页多题）。请识别图中【每一道】题：转写题干（数学符号尽量用 Unicode：√ ² ³ △ ⊥ ∠ ° π ≤ ≥ ≠ ∽ ≌，不要用反斜杠 LaTeX 命令如 \sqrt \triangle，以免 JSON 转义出错；忽略手写笔迹 / 批改 / 答案），并从给定知识点清单选最匹配的 id。

只输出一个 JSON 数组，每个元素是一道题：
[{"题号":"图中题号如 4 / 16，没有就空","read_text":"题干，数学符号用 Unicode 不用反斜杠","has_figure":true 或 false,"primary_kp_id":"清单中真实 id","primary_kp_name":"知识点名","alt_kp_ids":["备选id"],"confidence":0 到 1 的小数,"reason":"一句话归类依据"}]

只能用清单中真实存在的 id，绝不自造；尽量归到最细的「点」级，把握不大可只到「节」/「章」级并相应调低 confidence。不要写解释、不要用 markdown 代码块。

知识点清单（格式 id | 册 > 章 > 节 > 点）：
"#;

const REVIEW_HEADERS: [&str; 13] = [
    "图片",
    "题号",
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
        .timeout(std::time::Duration::from_secs(180));
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
                { "type": "text", "text": "识别这张图里的所有题目，按要求只输出 JSON 数组。" },
                { "type": "image_url", "image_url": { "url": data_uri, "detail": "high" } }
            ]}
        ],
        "temperature": 0,
        "max_tokens": 8000
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

/// 兜底：把 JSON 字符串里残留的非法反斜杠转义（`\` 后跟非合法转义字符，如 LaTeX 的 \s \p \( ）
/// 改成双反斜杠，让 serde 至少能解析。prompt 已要求用 Unicode，这里只是保险。
fn fix_json_escapes(s: &str) -> String {
    let valid = ['"', '\\', '/', 'b', 'f', 'n', 'r', 't', 'u'];
    let mut out = String::with_capacity(s.len() + 32);
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&next) if valid.contains(&next) => {
                    out.push('\\');
                    out.push(next);
                    chars.next();
                }
                _ => {
                    out.push('\\');
                    out.push('\\');
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// 从模型输出里抠出 JSON 数组，逐题转成 Question（含 id 校验）。
fn parse_questions(text: &str, id_index: &HashMap<String, KpNode>) -> Result<Vec<Question>, String> {
    let start = text.find('[').ok_or("输出里找不到 JSON 数组开头 [")?;
    let end = text.rfind(']').ok_or("输出里找不到 ]")?;
    if end <= start {
        return Err("JSON 数组边界异常".to_string());
    }
    let slice = &text[start..=end];
    let val: serde_json::Value = serde_json::from_str(slice)
        .or_else(|_| serde_json::from_str(&fix_json_escapes(slice)))
        .map_err(|e| format!("JSON 数组解析失败：{e}"))?;
    let items = val.as_array().ok_or("解析结果不是数组")?;

    let mut qs = Vec::new();
    for it in items {
        let pid = it["primary_kp_id"].as_str().unwrap_or("").to_string();
        let in_tree = id_index.contains_key(&pid);
        let path = id_index.get(&pid).map(|n| n.path.clone()).unwrap_or_default();
        let alt: Vec<String> = it["alt_kp_ids"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        qs.push(Question {
            label: it["题号"].as_str().unwrap_or("").to_string(),
            read_text: it["read_text"].as_str().unwrap_or("").to_string(),
            has_figure: it["has_figure"].as_bool().unwrap_or(false),
            primary_kp_id: pid,
            primary_kp_path: path,
            in_tree,
            alt_kp_ids: alt,
            confidence: it["confidence"].as_f64().unwrap_or(0.0),
            reason: it["reason"].as_str().unwrap_or("").to_string(),
        });
    }
    Ok(qs)
}

/// 识别一张图（可能多题）。失败不 panic，记进 error。
fn recognize_image(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    key: &str,
    model: &str,
    system_prompt: &str,
    id_index: &HashMap<String, KpNode>,
    image: &str,
) -> ImageResult {
    let mut r = ImageResult {
        image: image.to_string(),
        ok: false,
        error: String::new(),
        raw: String::new(),
        latency_sec: 0.0,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        questions: Vec::new(),
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

    match parse_questions(&raw, id_index) {
        Ok(qs) => {
            r.questions = qs;
            r.ok = true;
        }
        Err(e) => r.error = e,
    }
    r
}

fn print_image(r: &ImageResult) {
    println!("\n==== {} ====", r.image);
    if !r.ok {
        println!("失败：{}", r.error);
        return;
    }
    println!(
        "识别出 {} 道题（耗时 {:.1}s，tokens {}）",
        r.questions.len(),
        r.latency_sec,
        r.total_tokens
    );
    for (i, q) in r.questions.iter().enumerate() {
        let label = if q.label.is_empty() { "?" } else { q.label.as_str() };
        println!("\n--- 第 {} 道（题号 {}）---", i + 1, label);
        println!("读题：{}", q.read_text);
        let tree_mark = if q.in_tree { "" } else { "  (不在知识点树！)" };
        println!("归类：{}  {}{}", q.primary_kp_id, q.primary_kp_path, tree_mark);
        println!("备选：{:?}    置信度：{:.2}", q.alt_kp_ids, q.confidence);
    }
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

fn write_review_csv(results: &[ImageResult], path: &Path) -> Result<(), String> {
    let mut wtr = csv::Writer::from_path(path).map_err(|e| format!("建 review.csv 失败：{e}"))?;
    wtr.write_record(REVIEW_HEADERS).map_err(|e| format!("写表头失败：{e}"))?;
    for r in results {
        if !r.ok {
            let mut row = vec![r.image.clone(), String::new(), format!("[识别失败] {}", r.error)];
            row.resize(13, String::new());
            wtr.write_record(&row).map_err(|e| format!("写行失败：{e}"))?;
            continue;
        }
        for q in &r.questions {
            let pid_mark = if q.in_tree {
                q.primary_kp_id.clone()
            } else {
                format!("{} (不在树中)", q.primary_kp_id)
            };
            let row = vec![
                r.image.clone(),
                q.label.clone(),
                q.read_text.clone(),
                q.has_figure.to_string(),
                pid_mark,
                q.primary_kp_path.clone(),
                q.alt_kp_ids.join(" "),
                format!("{}", q.confidence),
                q.reason.clone(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ];
            wtr.write_record(&row).map_err(|e| format!("写行失败：{e}"))?;
        }
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
    let r = recognize_image(
        &ctx.client,
        &ctx.endpoint,
        &ctx.cfg.key,
        &ctx.cfg.model,
        &ctx.system_prompt,
        &ctx.loaded.id_index,
        image,
    );
    print_image(&r);
    Ok(())
}

fn cmd_run_dir(dir: &str) -> Result<(), String> {
    let ctx = prepare_run()?;
    let images = gather_images(dir)?;
    println!(
        "端点 {} | 模型 {} | 知识点 {} 个 | 待识别 {} 张图",
        ctx.endpoint,
        ctx.cfg.model,
        ctx.loaded.points.len(),
        images.len()
    );

    let mut results: Vec<ImageResult> = Vec::new();
    let t_start = Instant::now();
    for (i, img) in images.iter().enumerate() {
        print!("[{}/{}] {} ... ", i + 1, images.len(), img);
        std::io::stdout().flush().ok();
        let r = recognize_image(
            &ctx.client,
            &ctx.endpoint,
            &ctx.cfg.key,
            &ctx.cfg.model,
            &ctx.system_prompt,
            &ctx.loaded.id_index,
            img,
        );
        if r.ok {
            println!("ok {:.1}s，识别 {} 道题", r.latency_sec, r.questions.len());
        } else {
            println!("失败：{}", r.error.chars().take(80).collect::<String>());
        }
        results.push(r);
    }

    let out_dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../out"));
    fs::create_dir_all(out_dir).map_err(|e| format!("建输出目录失败：{e}"))?;

    let mut jsonl = String::new();
    for r in &results {
        let qs: Vec<serde_json::Value> = r
            .questions
            .iter()
            .map(|q| {
                serde_json::json!({
                    "题号": q.label, "read_text": q.read_text, "has_figure": q.has_figure,
                    "primary_kp_id": q.primary_kp_id, "primary_kp_path": q.primary_kp_path,
                    "in_tree": q.in_tree, "alt_kp_ids": q.alt_kp_ids,
                    "confidence": q.confidence, "reason": q.reason,
                })
            })
            .collect();
        let line = serde_json::json!({
            "image": r.image, "ok": r.ok, "error": r.error,
            "latency_sec": r.latency_sec, "total_tokens": r.total_tokens,
            "questions": qs,
        });
        jsonl.push_str(&line.to_string());
        jsonl.push('\n');
    }
    let jsonl_path = out_dir.join("results.jsonl");
    fs::write(&jsonl_path, &jsonl).map_err(|e| format!("写 results.jsonl 失败：{e}"))?;
    let review_path = out_dir.join("review.csv");
    write_review_csv(&results, &review_path)?;

    let img_ok = results.iter().filter(|r| r.ok).count();
    let img_fail = results.len() - img_ok;
    let total_q: usize = results.iter().map(|r| r.questions.len()).sum();
    let has_fig = results.iter().flat_map(|r| &r.questions).filter(|q| q.has_figure).count();
    let not_in_tree = results.iter().flat_map(|r| &r.questions).filter(|q| !q.in_tree).count();
    let total_tok: u64 = results.iter().map(|r| r.total_tokens).sum();

    println!("\n== 批量完成（耗时 {:.0}s）==", t_start.elapsed().as_secs_f64());
    println!("  图片 {}（成功 {img_ok} / 失败 {img_fail}）", results.len());
    println!("  共识别出 {total_q} 道题");
    println!("  含图形题 {has_fig} / 自造 id（不在树中）{not_in_tree}");
    println!("  合计 tokens {total_tok}");
    println!("  原始结果：{}", jsonl_path.display());
    println!("  人工评分表：{}", review_path.display());
    println!("\n下一步：打开 review.csv 逐行（每行一题）填「读题正确」「归类正确」，存盘后 score --review <review.csv>");
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

/// score：读人工填好的 review.csv（每行一题），算读题 / 归类准确率 + 置信度校准。
fn cmd_score(review: &str) -> Result<(), String> {
    let mut rdr = csv::Reader::from_path(review).map_err(|e| format!("读不到 {review}：{e}"))?;
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
    println!("  题目总数 {total}");
    if read_scores.is_empty() {
        println!("  读题准确率：无数据（请先填「读题正确」列）");
    } else {
        println!("  读题准确率：{:.1}%（已评 {} 题，0.5 计部分正确）", mean(&read_scores) * 100.0, read_scores.len());
    }
    if cls_scores.is_empty() {
        println!("  归类准确率：无数据（请先填「归类正确」列）");
    } else {
        println!("  归类准确率：{:.1}%（已评 {} 题）", mean(&cls_scores) * 100.0, cls_scores.len());
    }
    if !conf_right.is_empty() || !conf_wrong.is_empty() {
        let show = |v: &[f64]| if v.is_empty() { "无".to_string() } else { format!("{:.2}", mean(v)) };
        println!("  置信度校准参考：归类对时均置信度 {}，归类错时 {}", show(&conf_right), show(&conf_wrong));
    }
    if ungraded > 0 {
        println!("  还有 {ungraded} 题两列都没填，未计入。");
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
        "" => Err("用法：recognize-rs <check | run --image 图 | run --dir 目录 | score --review csv>".to_string()),
        other => Err(format!("未知子命令：{other}")),
    };

    if let Err(e) = result {
        eprintln!("错误：{e}");
        process::exit(1);
    }
}
