#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
student-kpscan 第 0 步识别率 spike 脚本。

目的：在不碰 Tauri / MVP 的前提下，验证「调 newapi 网关（OpenAI 兼容）的 vision 模型
读印刷体数学题 + 按知识点树归类」这条唯一的硬不确定性链路能不能跑通、准不准。

测两个数：
  1. 读题准确率：印刷体题目文本（含公式 / 几何图）转写得对不对。
  2. 归类准确率：归到的知识点 id 对不对。
这两个数都需要人工 ground truth（老师 / 家长核对），所以脚本分两步：
  run   先让模型逐图产出「读题文本 + 归类」，写出人工评分表 review.csv。
  score 读人工填好的 review.csv，算出读题准确率 + 归类准确率。

硬约束：识别一律走 newapi 的 OpenAI 兼容端点，不直连厂商；key 从环境变量 / .env 读，绝不写进代码或提交。

用法：
  python recognize.py check                      # 校验配置 + 知识点树，不调 API
  python recognize.py run --image 某张错题.jpg    # 单图，先跑通链路（第一步做这个）
  python recognize.py run --dir 错题目录/         # 批量，写 results.jsonl + review.csv
  python recognize.py score --review out/review.csv   # 人工填表后算两个准确率
"""

import argparse
import base64
import csv
import json
import os
import sys
import time
from pathlib import Path

try:
    import requests
except ImportError:
    sys.exit("缺少依赖 requests。请在 spike/ 下执行：pip install -r requirements.txt")
try:
    import yaml
except ImportError:
    sys.exit("缺少依赖 PyYAML。请在 spike/ 下执行：pip install -r requirements.txt")


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
DEFAULT_KP_TREE = REPO_ROOT / "knowledge" / "zhejiang-math-kp-v0.yaml"
DEFAULT_OUT = SCRIPT_DIR / "out"

IMAGE_EXTS = {".jpg", ".jpeg", ".png", ".webp", ".bmp", ".gif"}
MIME_BY_EXT = {
    ".jpg": "image/jpeg",
    ".jpeg": "image/jpeg",
    ".png": "image/png",
    ".webp": "image/webp",
    ".bmp": "image/bmp",
    ".gif": "image/gif",
}

REVIEW_HEADERS = [
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
]


# --------------------------------------------------------------------------- #
# 配置：.env 读取 + 解析顺序（CLI 参数 > 环境变量 > .env 文件 > 默认）
# --------------------------------------------------------------------------- #
def load_dotenv():
    """读 spike/.env 与当前目录 .env 到 os.environ（不覆盖已有的，shell 里设的优先）。"""
    for env_path in (SCRIPT_DIR / ".env", Path.cwd() / ".env"):
        if not env_path.is_file():
            continue
        for raw in env_path.read_text(encoding="utf-8").splitlines():
            line = raw.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, val = line.split("=", 1)
            key = key.strip()
            val = val.strip().strip('"').strip("'")
            # 去掉行尾注释（# 前需有空格，避免误伤 key 里的 #）
            if " #" in val:
                val = val.split(" #", 1)[0].strip()
            if key and key not in os.environ:
                os.environ[key] = val


def resolve_config(args):
    """解析 base_url / key / model，返回 (endpoint, key, model)。缺关键项直接报错退出。"""
    base_url = args.base_url or os.environ.get("NEWAPI_BASE_URL", "")
    key = args.key or os.environ.get("NEWAPI_KEY", "")
    model = args.model or os.environ.get("NEWAPI_MODEL", "")

    missing = []
    if not base_url:
        missing.append("NEWAPI_BASE_URL（newapi 端点，如 https://你的newapi/v1）")
    if not key:
        missing.append("NEWAPI_KEY（newapi 令牌 sk-...）")
    if not model:
        missing.append("NEWAPI_MODEL（vision 模型名，如 doubao-vision-pro-32k）")
    if missing:
        print("配置缺失，请在 spike/.env 填写（参考 .env.example），或用命令行参数覆盖：", file=sys.stderr)
        for m in missing:
            print("  - " + m, file=sys.stderr)
        sys.exit(2)

    return normalize_endpoint(base_url), key, model


def normalize_endpoint(base_url):
    """把各种写法的 base_url 归一到 .../chat/completions。"""
    url = base_url.rstrip("/")
    if url.endswith("/chat/completions"):
        return url
    if url.endswith("/v1"):
        return url + "/chat/completions"
    return url + "/v1/chat/completions"


# --------------------------------------------------------------------------- #
# 知识点树：加载、展平、校验
# --------------------------------------------------------------------------- #
def vol_short(grade, volume):
    g = {"七年级": "七", "八年级": "八", "九年级": "九"}.get(grade, grade)
    v = {"上册": "上", "下册": "下"}.get(volume, volume)
    return g + v


def load_kp_tree(path):
    """加载知识点树。返回 dict：meta / points(叶子) / id_index(全层级) / issues(结构问题)。"""
    path = Path(path)
    if not path.is_file():
        sys.exit("找不到知识点树：%s" % path)
    data = yaml.safe_load(path.read_text(encoding="utf-8"))

    meta = data.get("meta", {})
    points = []          # 叶子：[{id, name, path}]
    id_index = {}        # 全层级 id -> {name, path, level}
    issues = []
    seen = set()

    def register(_id, name, full_path, level):
        if _id in seen:
            issues.append("重复 id：%s" % _id)
        seen.add(_id)
        id_index[_id] = {"name": name, "path": full_path, "level": level}

    for vol in data.get("volumes", []):
        vshort = vol_short(vol.get("grade", ""), vol.get("volume", ""))
        register(vol["id"], vshort, vshort, "册")
        for chap in vol.get("chapters", []):
            cpath = "%s > %s" % (vshort, chap["name"])
            register(chap["id"], chap["name"], cpath, "章")
            if not chap["id"].startswith(vol["id"] + "."):
                issues.append("章 id 前缀不符：%s 不在册 %s 下" % (chap["id"], vol["id"]))
            for sec in chap.get("sections", []):
                spath = "%s > %s" % (cpath, sec["name"])
                register(sec["id"], sec["name"], spath, "节")
                if not sec["id"].startswith(chap["id"] + "."):
                    issues.append("节 id 前缀不符：%s 不在章 %s 下" % (sec["id"], chap["id"]))
                for pt in sec.get("points", []):
                    ppath = "%s > %s" % (spath, pt["name"])
                    register(pt["id"], pt["name"], ppath, "点")
                    if not pt["id"].startswith(sec["id"] + "."):
                        issues.append("点 id 前缀不符：%s 不在节 %s 下" % (pt["id"], sec["id"]))
                    points.append({"id": pt["id"], "name": pt["name"], "path": ppath})

    return {"meta": meta, "points": points, "id_index": id_index, "issues": issues}


def count_levels(id_index):
    c = {"册": 0, "章": 0, "节": 0, "点": 0}
    for v in id_index.values():
        c[v["level"]] = c.get(v["level"], 0) + 1
    return c


# --------------------------------------------------------------------------- #
# Prompt 构造
# --------------------------------------------------------------------------- #
def build_system_prompt(points):
    """喂给模型的 system prompt：任务说明 + 输出 JSON 约定 + 展平的知识点清单。"""
    lines = ["%s | %s" % (p["id"], p["path"]) for p in points]
    kp_block = "\n".join(lines)
    return (
        "你是浙江初中数学错题识别助手。我会给你一张错题图片（机器印刷体题目，可能含数学公式和几何图 / 函数图象）。"
        "请完成两件事，并且只输出一个 JSON 对象，不要写任何解释、不要用 markdown 代码块。\n\n"
        "任务一，读题：转写图片中的印刷体数学题目文本。公式用 LaTeX（行内写成 $...$）。"
        "忽略手写笔迹、红叉、老师批改等非题干内容。若图中有多道题，转写最完整的一道。\n\n"
        "任务二，归类：从下面给定的知识点清单中，选出该题最匹配的 1 个知识点 id 作为 primary_kp_id，"
        "可再给最多 2 个备选放进 alt_kp_ids。只能选清单中真实存在的 id，绝不要自造 id。"
        "尽量归到最细的「点」级 id；若把握不大，可以只给到「节」级（去掉 id 最后一段，如 8A.2.7）"
        "或「章」级（如 8A.2），并相应调低 confidence。\n\n"
        "输出 JSON，字段如下：\n"
        "{\n"
        '  "read_text": "转写的题目文本，公式用 LaTeX",\n'
        '  "has_figure": true 或 false,\n'
        '  "primary_kp_id": "如 8A.2.7.1",\n'
        '  "primary_kp_name": "该 id 对应的知识点名称",\n'
        '  "alt_kp_ids": ["备选id", "..."],\n'
        '  "confidence": 0 到 1 之间的小数,\n'
        '  "reason": "一句话归类依据"\n'
        "}\n\n"
        "知识点清单（格式为：id | 册 > 章 > 节 > 点）：\n"
        + kp_block
    )


# --------------------------------------------------------------------------- #
# 图片编码 + 调 API
# --------------------------------------------------------------------------- #
def encode_image(path):
    path = Path(path)
    mime = MIME_BY_EXT.get(path.suffix.lower(), "image/jpeg")
    b64 = base64.b64encode(path.read_bytes()).decode("ascii")
    return "data:%s;base64,%s" % (mime, b64)


def call_vision(endpoint, key, model, system_prompt, data_uri, timeout=120, verbose=False):
    """调 OpenAI 兼容 vision 端点。返回 (content_text, usage_dict, latency_sec)。失败抛异常。"""
    payload = {
        "model": model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": "请识别这张错题图，并按要求只输出 JSON。"},
                    {"type": "image_url", "image_url": {"url": data_uri, "detail": "high"}},
                ],
            },
        ],
        "temperature": 0,
        "max_tokens": 1500,
    }
    headers = {"Authorization": "Bearer " + key, "Content-Type": "application/json"}

    if verbose:
        print("    POST %s  model=%s  图片字节(base64)=%d" % (endpoint, model, len(data_uri)))

    t0 = time.time()
    resp = requests.post(endpoint, headers=headers, json=payload, timeout=timeout)
    latency = time.time() - t0

    if resp.status_code != 200:
        # newapi 的报错通常在 body 里写得很清楚（渠道未配 / 模型名错 / 余额等），原样抛出便于排查。
        raise RuntimeError("HTTP %d：%s" % (resp.status_code, resp.text[:1000]))

    data = resp.json()
    choices = data.get("choices") or []
    if not choices:
        raise RuntimeError("响应无 choices：%s" % json.dumps(data, ensure_ascii=False)[:500])
    content = choices[0].get("message", {}).get("content", "")
    if isinstance(content, list):  # 个别实现返回分段 content
        content = "".join(seg.get("text", "") for seg in content if isinstance(seg, dict))
    usage = data.get("usage", {}) or {}
    return content, usage, latency


def parse_model_json(text):
    """从模型输出里抠出 JSON 对象。容忍 markdown 代码块和前后多余文字。"""
    t = (text or "").strip()
    start, end = t.find("{"), t.rfind("}")
    if start == -1 or end == -1 or end <= start:
        raise ValueError("输出里找不到 JSON 对象")
    return json.loads(t[start : end + 1])


# --------------------------------------------------------------------------- #
# 子命令：check
# --------------------------------------------------------------------------- #
def cmd_check(args):
    print("== 配置检查 ==")
    base_url = args.base_url or os.environ.get("NEWAPI_BASE_URL", "")
    key = os.environ.get("NEWAPI_KEY", "") or (args.key or "")
    model = args.model or os.environ.get("NEWAPI_MODEL", "")
    print("  NEWAPI_BASE_URL：%s" % (normalize_endpoint(base_url) if base_url else "（未设置）"))
    print("  NEWAPI_KEY：%s" % ("已设置（%d 位，已隐藏）" % len(key) if key else "（未设置）"))
    print("  NEWAPI_MODEL：%s" % (model or "（未设置）"))
    if not (base_url and key and model):
        print("  提示：三项需在 spike/.env 配齐后才能 run。check 不调用 API。")

    print("\n== 知识点树检查：%s ==" % args.kp_tree)
    tree = load_kp_tree(args.kp_tree)
    c = count_levels(tree["id_index"])
    print("  册 %d / 章 %d / 节 %d / 点 %d（叶子知识点 %d 个）"
          % (c["册"], c["章"], c["节"], c["点"], len(tree["points"])))
    print("  meta.version=%s  status=%s" % (tree["meta"].get("version"), tree["meta"].get("status")))
    if tree["issues"]:
        print("  结构问题 %d 处：" % len(tree["issues"]))
        for s in tree["issues"]:
            print("    - " + s)
    else:
        print("  结构自洽：id 唯一、层级前缀一致。")

    if args.show_prompt:
        print("\n== system prompt 预览（前 1200 字） ==")
        print(build_system_prompt(tree["points"])[:1200] + " ……")


# --------------------------------------------------------------------------- #
# 子命令：run
# --------------------------------------------------------------------------- #
def gather_images(args):
    if args.image:
        p = Path(args.image)
        if not p.is_file():
            sys.exit("找不到图片：%s" % p)
        return [p]
    d = Path(args.dir)
    if not d.is_dir():
        sys.exit("找不到目录：%s" % d)
    imgs = sorted(p for p in d.iterdir() if p.suffix.lower() in IMAGE_EXTS)
    if args.limit:
        imgs = imgs[: args.limit]
    if not imgs:
        sys.exit("目录里没有图片（支持 %s）：%s" % ("/".join(sorted(IMAGE_EXTS)), d))
    return imgs


def recognize_one(img, endpoint, key, model, system_prompt, id_index, verbose):
    """识别单图，返回一条结果 dict（含原始输出、解析结果、是否成功）。"""
    rec = {"图片": img.name, "路径": str(img)}
    try:
        data_uri = encode_image(img)
        raw, usage, latency = call_vision(endpoint, key, model, system_prompt, data_uri, verbose=verbose)
        rec["latency_sec"] = round(latency, 2)
        rec["usage"] = usage
        rec["raw"] = raw
        parsed = parse_model_json(raw)
        rec["parsed"] = parsed
        pid = parsed.get("primary_kp_id", "")
        rec["primary_kp_id"] = pid
        rec["primary_kp_path"] = id_index.get(pid, {}).get("path", "")
        rec["id_in_tree"] = pid in id_index  # 模型有没有自造 id
        rec["ok"] = True
    except Exception as e:
        rec["ok"] = False
        rec["error"] = str(e)
    return rec


def print_single(rec):
    print("\n==== 识别结果：%s ====" % rec["图片"])
    if not rec["ok"]:
        print("失败：%s" % rec["error"])
        return
    p = rec["parsed"]
    print("读题文本：\n%s" % p.get("read_text", ""))
    print("\n含图形：%s" % p.get("has_figure"))
    print("归类（主）：%s  %s" % (rec["primary_kp_id"], rec["primary_kp_path"] or "(不在知识点树中！)"))
    print("备选：%s" % p.get("alt_kp_ids"))
    print("置信度：%s" % p.get("confidence"))
    print("依据：%s" % p.get("reason"))
    if not rec["id_in_tree"]:
        print("⚠ 模型给的 primary_kp_id 不在知识点树里，可能是自造 id，归类不可信。")
    u = rec.get("usage", {})
    print("\n耗时 %ss，tokens 提示 %s / 生成 %s"
          % (rec.get("latency_sec"), u.get("prompt_tokens", "?"), u.get("completion_tokens", "?")))


def write_review_csv(results, out_csv):
    with open(out_csv, "w", encoding="utf-8-sig", newline="") as f:
        w = csv.writer(f)
        w.writerow(REVIEW_HEADERS)
        for r in results:
            if r["ok"]:
                p = r["parsed"]
                w.writerow([
                    r["图片"],
                    p.get("read_text", ""),
                    p.get("has_figure", ""),
                    r["primary_kp_id"] + ("" if r["id_in_tree"] else " (不在树中)"),
                    r["primary_kp_path"],
                    " ".join(p.get("alt_kp_ids", []) or []),
                    p.get("confidence", ""),
                    p.get("reason", ""),
                    "", "", "", "",  # 人工评分列，留空待填
                ])
            else:
                w.writerow([r["图片"], "[识别失败] " + r.get("error", ""), "", "", "", "", "", "", "", "", "", ""])


def cmd_run(args):
    endpoint, key, model = resolve_config(args)
    tree = load_kp_tree(args.kp_tree)
    system_prompt = build_system_prompt(tree["points"])
    id_index = tree["id_index"]
    imgs = gather_images(args)

    print("端点 %s | 模型 %s | 知识点 %d 个 | 待识别图片 %d 张"
          % (endpoint, model, len(tree["points"]), len(imgs)))

    # 单图：先跑通链路，直接打印，不落盘。
    if args.image:
        rec = recognize_one(imgs[0], endpoint, key, model, system_prompt, id_index, args.verbose)
        print_single(rec)
        return

    # 批量：逐图识别，写 results.jsonl + review.csv。
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    results = []
    t_start = time.time()
    for i, img in enumerate(imgs, 1):
        print("[%d/%d] %s ..." % (i, len(imgs), img.name), end=" ", flush=True)
        rec = recognize_one(img, endpoint, key, model, system_prompt, id_index, args.verbose)
        if rec["ok"]:
            print("ok %ss  -> %s（置信度 %s）"
                  % (rec.get("latency_sec"), rec["primary_kp_id"], rec["parsed"].get("confidence")))
        else:
            print("失败：%s" % rec["error"][:120])
        results.append(rec)

    jsonl_path = out_dir / "results.jsonl"
    with open(jsonl_path, "w", encoding="utf-8") as f:
        for r in results:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    review_path = out_dir / "review.csv"
    write_review_csv(results, review_path)

    ok = [r for r in results if r["ok"]]
    fail = [r for r in results if not r["ok"]]
    not_in_tree = [r for r in ok if not r["id_in_tree"]]
    has_fig = [r for r in ok if r["parsed"].get("has_figure")]
    avg_lat = round(sum(r["latency_sec"] for r in ok) / len(ok), 2) if ok else 0
    total_tok = sum((r.get("usage") or {}).get("total_tokens", 0) for r in ok)

    print("\n== 批量完成（耗时 %ds） ==" % int(time.time() - t_start))
    print("  成功 %d / 失败 %d" % (len(ok), len(fail)))
    print("  含图形题 %d（公式 / 几何图是识别难点，重点核这些）" % len(has_fig))
    print("  自造 id（不在树中）%d" % len(not_in_tree))
    print("  平均耗时 %ss / 图，合计 tokens %d" % (avg_lat, total_tok))
    print("  原始结果：%s" % jsonl_path)
    print("  人工评分表：%s" % review_path)
    print("\n下一步：打开 review.csv，逐行填「读题正确」「归类正确」两列（归类错时可在「正确知识点ID」填对的 id），")
    print("       存盘后执行：python recognize.py score --review %s" % review_path)


# --------------------------------------------------------------------------- #
# 子命令：score
# --------------------------------------------------------------------------- #
def _to_float(s):
    s = (s or "").strip()
    if s == "":
        return None
    try:
        return float(s)
    except ValueError:
        return None


def cmd_score(args):
    path = Path(args.review)
    if not path.is_file():
        sys.exit("找不到评分表：%s" % path)
    with open(path, encoding="utf-8-sig", newline="") as f:
        rows = list(csv.DictReader(f))
    if not rows:
        sys.exit("评分表是空的：%s" % path)

    read_col = "读题正确(1=对/0.5=部分/0=错)"
    cls_col = "归类正确(1/0)"
    conf_col = "置信度"

    read_scores, cls_scores = [], []
    conf_when_cls_right, conf_when_cls_wrong = [], []
    for r in rows:
        rv = _to_float(r.get(read_col))
        if rv is not None:
            read_scores.append(rv)
        cv = _to_float(r.get(cls_col))
        if cv is not None:
            cls_scores.append(cv)
            conf = _to_float(r.get(conf_col))
            if conf is not None:
                (conf_when_cls_right if cv >= 1 else conf_when_cls_wrong).append(conf)

    total = len(rows)
    print("== 识别率 spike 评分：%s ==" % path)
    print("  图片总数 %d" % total)

    if read_scores:
        acc = sum(read_scores) / len(read_scores)
        print("  读题准确率：%.1f%%（已评 %d 张，0.5 计部分正确）" % (acc * 100, len(read_scores)))
    else:
        print("  读题准确率：无数据（请先填「%s」列）" % read_col)

    if cls_scores:
        acc = sum(cls_scores) / len(cls_scores)
        print("  归类准确率：%.1f%%（已评 %d 张）" % (acc * 100, len(cls_scores)))
    else:
        print("  归类准确率：无数据（请先填「%s」列）" % cls_col)

    if conf_when_cls_right or conf_when_cls_wrong:
        def avg(xs):
            return sum(xs) / len(xs) if xs else float("nan")
        print("  模型置信度校准参考：归类对时均置信度 %.2f，归类错时 %.2f"
              % (avg(conf_when_cls_right), avg(conf_when_cls_wrong)))

    ungraded = sum(1 for r in rows if _to_float(r.get(read_col)) is None and _to_float(r.get(cls_col)) is None)
    if ungraded:
        print("  还有 %d 张两列都没填，未计入。" % ungraded)
    print("\n  通过线由 spike 结果与需求方拍定后再判定是否进入 MVP 开发。")


# --------------------------------------------------------------------------- #
# 入口
# --------------------------------------------------------------------------- #
def build_parser():
    p = argparse.ArgumentParser(
        description="student-kpscan 识别率 spike：调 newapi vision 读数学错题 + 归类知识点。",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    sub = p.add_subparsers(dest="cmd", required=True)

    def add_common(sp):
        sp.add_argument("--base-url", help="newapi 端点（默认读 NEWAPI_BASE_URL）")
        sp.add_argument("--key", help="newapi 令牌（默认读 NEWAPI_KEY，建议用环境变量别走命令行）")
        sp.add_argument("--model", help="vision 模型名（默认读 NEWAPI_MODEL）")
        sp.add_argument("--kp-tree", default=str(DEFAULT_KP_TREE), help="知识点树 YAML 路径")

    sp_check = sub.add_parser("check", help="校验配置 + 知识点树，不调 API")
    add_common(sp_check)
    sp_check.add_argument("--show-prompt", action="store_true", help="顺便预览喂给模型的 system prompt")

    sp_run = sub.add_parser("run", help="识别单图或整个目录")
    add_common(sp_run)
    g = sp_run.add_mutually_exclusive_group(required=True)
    g.add_argument("--image", help="单张错题图（先用这个跑通链路）")
    g.add_argument("--dir", help="错题图目录（批量）")
    sp_run.add_argument("--out", default=str(DEFAULT_OUT), help="批量结果输出目录（默认 spike/out）")
    sp_run.add_argument("--limit", type=int, help="批量时只跑前 N 张（试手用）")
    sp_run.add_argument("--verbose", action="store_true", help="打印请求细节")

    sp_score = sub.add_parser("score", help="读人工填好的 review.csv，算读题 / 归类准确率")
    sp_score.add_argument("--review", required=True, help="人工填好的 review.csv 路径")

    return p


def main():
    load_dotenv()
    args = build_parser().parse_args()
    if args.cmd == "check":
        cmd_check(args)
    elif args.cmd == "run":
        cmd_run(args)
    elif args.cmd == "score":
        cmd_score(args)


if __name__ == "__main__":
    main()
