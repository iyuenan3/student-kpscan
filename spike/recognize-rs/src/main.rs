//! student-kpscan 识别率 spike 的 Rust 版（recognize-rs）。
//! 对照 spike/recognize.py 逐步移植：check / run / score。
//! 第四步：run --image 端到端，调 newapi 网关 vision 读题 + 按知识点树归类 + id 校验。
#![allow(dead_code)] // KpNode.name、PointRef.name 暂未读，保留语义完整

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process;
use std::time::Duration;

use base64::prelude::*; // 提供 BASE64_STANDARD 和 Engine trait
use serde::Deserialize;

// ===== LLM 配置 =====
struct Config {
    base_url: String,
    key: String,
    model: String,
    ca_cert: String, // 自签 CA 路径，可空
}

// ===== 知识点树结构（对应 YAML 各层）=====
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

// ===== system prompt 固定正文（与 Python 版一致，照搬保证行为相同）=====
// 用 raw string r#"..."#：里面的引号和 JSON 的 { } 都不必转义。
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

/// 册码简称：七年级上册 -> 七上。
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

/// 子 id 是否挂在父 id 下（child 形如 parent.xxx）。
fn is_child(child_id: &str, parent_id: &str) -> bool {
    child_id.starts_with(parent_id) && child_id[parent_id.len()..].starts_with('.')
}

/// 登记一个节点 + 查重。独立函数显式传 &mut，避免闭包重复可变借用。
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

/// 读 spike/.env 的字段。
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

/// 解析 YAML 成 KpTree。
fn load_kp_tree() -> Result<KpTree, String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../knowledge/zhejiang-math-kp-v0.yaml");
    let text = fs::read_to_string(path).map_err(|e| format!("读不到知识点树 {path}：{e}"))?;
    serde_yaml::from_str(&text).map_err(|e| format!("知识点树 YAML 解析失败：{e}"))
}

/// 遍历树建 id_index、收集叶子、前缀校验。
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

/// 把 base_url 归一到 .../chat/completions。
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

/// 拼 system prompt：固定正文 + 展平的知识点清单（id | path 每行一条）。
fn build_system_prompt(points: &[PointRef]) -> String {
    let kp_block: String = points
        .iter()
        .map(|p| format!("{} | {}", p.id, p.path))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{}{}", SYSTEM_PROMPT_HEAD, kp_block)
}

/// 图片读成 base64 data URI。
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

/// 构建 HTTP 客户端：禁代理 + 视情况加载自签 CA。
fn build_client(cfg: &Config) -> Result<reqwest::blocking::Client, String> {
    // no_proxy 直接在代码里绕开环境注入的坏代理（网关是 IP 直连，不需代理）。
    let mut builder = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(120));
    if !cfg.ca_cert.is_empty() {
        let pem = fs::read(&cfg.ca_cert).map_err(|e| format!("读不到 CA 证书 {}：{e}", cfg.ca_cert))?;
        let cert = reqwest::Certificate::from_pem(&pem).map_err(|e| format!("CA 证书解析失败：{e}"))?;
        builder = builder.add_root_certificate(cert);
    }
    builder.build().map_err(|e| format!("构建 HTTP 客户端失败：{e}"))
}

/// 调 OpenAI 兼容 vision 端点，返回模型输出的文本。
fn call_vision(
    client: &reqwest::blocking::Client,
    endpoint: &str,
    key: &str,
    model: &str,
    system_prompt: &str,
    data_uri: &str,
) -> Result<String, String> {
    // json! 宏：像写字面 JSON 一样构造请求体。
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
        Some(c) => Ok(c.to_string()),
        None => {
            let snippet: String = body.chars().take(300).collect();
            Err(format!("响应里没有 choices[0].message.content：{snippet}"))
        }
    }
}

/// 从模型输出里抠出 JSON 对象（容忍 markdown 包裹和前后多余文字）。
fn parse_model_json(text: &str) -> Result<serde_json::Value, String> {
    let start = text.find('{').ok_or("输出里找不到 {")?;
    let end = text.rfind('}').ok_or("输出里找不到 }")?;
    if end <= start {
        return Err("JSON 边界异常".to_string());
    }
    serde_json::from_str(&text[start..=end]).map_err(|e| format!("JSON 解析失败：{e}"))
}

/// check：校验配置 + 加载树 + 建索引 + 结构自洽报告（不调 API）。
fn cmd_check() -> Result<(), String> {
    let cfg = load_config()?;
    println!("== 配置检查 ==");
    println!("  NEWAPI_BASE_URL：{}", cfg.base_url);
    println!("  NEWAPI_KEY：已设置（{} 位，已隐藏）", cfg.key.len());
    println!("  NEWAPI_MODEL：{}", cfg.model);
    println!("  NEWAPI_CA_CERT：{}", if cfg.ca_cert.is_empty() { "(未设置，用系统 CA)" } else { cfg.ca_cert.as_str() });

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

/// run --image：单图端到端识别。
fn cmd_run(image: &str) -> Result<(), String> {
    let cfg = load_config()?;
    let tree = load_kp_tree()?;
    let loaded = build_index(&tree);
    let endpoint = normalize_endpoint(&cfg.base_url);
    let system_prompt = build_system_prompt(&loaded.points);
    let client = build_client(&cfg)?;
    let data_uri = encode_image(image)?;

    println!(
        "端点 {endpoint} | 模型 {} | 知识点 {} 个 | 图 {image}",
        cfg.model,
        loaded.points.len()
    );

    let raw = call_vision(&client, &endpoint, &cfg.key, &cfg.model, &system_prompt, &data_uri)?;
    let parsed = parse_model_json(&raw)?;

    let pid = parsed["primary_kp_id"].as_str().unwrap_or("");
    let in_tree = loaded.id_index.contains_key(pid);
    let path = loaded
        .id_index
        .get(pid)
        .map(|n| n.path.as_str())
        .unwrap_or("(不在知识点树中！)");

    println!("\n==== 识别结果：{image} ====");
    println!("读题文本：\n{}", parsed["read_text"].as_str().unwrap_or(""));
    println!("\n含图形：{}", parsed["has_figure"]);
    println!("归类（主）：{pid}  {path}");
    println!("备选：{}", parsed["alt_kp_ids"]);
    println!("置信度：{}", parsed["confidence"]);
    println!("依据：{}", parsed["reason"].as_str().unwrap_or(""));
    if !in_tree {
        println!("⚠ primary_kp_id 不在知识点树里，可能自造 id，归类不可信。");
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("");

    let result = match cmd {
        "check" => cmd_check(),
        "run" => match (args.get(2).map(String::as_str), args.get(3)) {
            (Some("--image"), Some(path)) => cmd_run(path),
            _ => Err("用法：recognize-rs run --image <图片路径>".to_string()),
        },
        "score" => Err("score 子命令待实现".to_string()),
        "" => Err("用法：recognize-rs <check|run --image 图|score>".to_string()),
        other => Err(format!("未知子命令：{other}")),
    };

    if let Err(e) = result {
        eprintln!("错误：{e}");
        process::exit(1);
    }
}
