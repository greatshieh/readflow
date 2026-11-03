#!/usr/bin/env python3
"""生成 ReadFlow 信息流向图 PNG（不依赖 SVG 渲染器，直接像素级绘制）。

用 Pillow 逐层画：背景 → 卡片 → 文本 → 箭头。之所以不用 SVG，
是因为本机 ImageMagick 的 SVG 后端对 filter/渐变/圆角矩形支持不全，
渲染出来会丢块、掉色。直接画像素可以在任意机器上得到同一张图。

依赖：Pillow + Noto Sans CJK（系统字体）
用法：python make_flow_png.py
"""

import os
from PIL import Image, ImageDraw, ImageFont

SCALE = 2                      # 超采样倍数，最终以 1/2 输出，得到高清图
W, H = 1600, 1000              # 逻辑尺寸
BG = "#fbfbfd"

# ── 配色（与项目主题一致：紫罗兰主色 + 语义色）─────────────────────
C_TEXT = "#2b2740"
C_TEXT2 = "#5c5675"
C_TEXT3 = "#8b87a3"
C_LINE = "#e2dfee"
C_WHITE = "#ffffff"

STAGES = [
    {
        "no": "01", "name": "输入", "sub": "你拿进来源",
        "accent": "#4a7be0", "tint": "#eef1ff", "border": "#dfe5f8",
        "ui": "界面：订阅栏 · 设置 → 常规 / 数据",
        "items": [
            ("订阅源（RSS / Atom / JSON Feed）", None),
            ("RSSHub 路由", "失败时自动换实例，真抓通才写库"),
            ("OPML 导入", "从别的阅读器迁过来走这条"),
            ("行情与财务", "新浪快照（免配置）/ Tushare"),
            ("过滤规则", "入库前先挡掉噪音源的垃圾条目"),
        ],
    },
    {
        "no": "02", "name": "阅读", "sub": "你读懂它",
        "accent": "#7c6ff0", "tint": "#f2efff", "border": "#e4dff8",
        "ui": "界面：三栏 · 正文工具条 · 两个悬浮按钮",
        "items": [
            ("高亮与笔记", "选中文字即浮出调色板"),
            ("AI 摘要 / 翻译", None),
            ("就本文追问（多轮）", None),
            ("文内目录 TOC", None),
            ("导出到 Obsidian", None),
            ("命令面板 Ctrl/⌘ + K", "先搜标题，无命中才退到全文"),
        ],
    },
    {
        "no": "03", "name": "沉淀", "sub": "你形成判断",
        "accent": "#2e9c76", "tint": "#e8f7f1", "border": "#d9ece4",
        "ui": "界面：研究工作台 · 复盘视图",
        "items": [
            ("研究事件", "带情感倾向 + 事件价格反应"),
            ("挂到论点", "反驳 / 存疑 / 支持，反驳置顶"),
            ("置信与状态变更", "改一次就得写一句理由"),
            ("覆盖清单", "必须写「退出条件」"),
            ("复盘（W）", None),
        ],
    },
    {
        "no": "04", "name": "输出", "sub": "你写出去",
        "accent": "#c48329", "tint": "#fdf1e3", "border": "#f4e6d2",
        "ui": "界面：报告页签 · 覆盖「生成一页」",
        "items": [
            ("周期报告", "按时间切窗：日报 / 周报 / 月报"),
            ("研究报告", "按实体聚合事件"),
            ("组装一页", "判断 + 证据 + 引用，markdown"),
            ("导出", "md / HTML / Obsidian Vault"),
            ("粘进 Obsidian 排版定稿", None),
        ],
    },
]

# ── 字体 ────────────────────────────────────────────────────────────
FONT_DIRS = [
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Medium.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
]


def find_font(bold: bool = False) -> str:
    """挑一个可用的中文字体文件，优先 Noto Sans CJK SC。"""
    cands = [
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc" if bold
        else "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Medium.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSerifCJK-Bold.ttc",
    ]
    for p in cands:
        if os.path.exists(p):
            return p
    raise RuntimeError("找不到 Noto CJK 字体，请先安装 fonts-noto-cjk")


F_REG = find_font(False)
F_BOLD = find_font(True)


def font(size: int, bold: bool = False):
    """按逻辑字号取字体（内部按 SCALE 放大）。"""
    return ImageFont.truetype(F_BOLD if bold else F_REG, int(size * SCALE))


def S(v):
    """逻辑坐标 → 像素坐标。"""
    return int(round(v * SCALE))


img = Image.new("RGB", (S(W), S(H)), BG)
d = ImageDraw.Draw(img)


def text(x, y, s, size=16, color=C_TEXT, bold=False, anchor="la"):
    d.text((S(x), S(y)), s, font=font(size, bold), fill=color, anchor=anchor)


def rrect(x, y, w, h, r, fill=None, outline=None, width=1):
    d.rounded_rectangle(
        [S(x), S(y), S(x + w), S(y + h)],
        radius=S(r), fill=fill, outline=outline, width=max(1, S(width)),
    )


# ══ 1. 标题 ══════════════════════════════════════════════════════
text(80, 54, "ReadFlow 的信息流向", 40, C_TEXT, bold=True)
text(80, 108, "一条信息从抓到手里，推到能写进报告的四个阶段", 18, C_TEXT3)
d.line([S(80), S(146), S(1520), S(146)], fill=C_LINE, width=S(1))

# ══ 2. 四个阶段 ══════════════════════════════════════════════════
CARD_Y, CARD_H, CARD_W, GAP = 226, 396, 336, 52
xs = [80 + i * (CARD_W + GAP) for i in range(4)]

for i, st in enumerate(STAGES):
    x = xs[i]

    # 阶段号 + 名称（在卡片上方）
    d.ellipse([S(x), S(178), S(x + 34), S(212)], fill=st["tint"])
    text(x + 17, 195, st["no"], 15, st["accent"], bold=True, anchor="mm")
    text(x + 44, 195, st["name"], 21, C_TEXT, bold=True, anchor="lm")

    # 卡片主体
    rrect(x, CARD_Y, CARD_W, CARD_H, 16, fill=C_WHITE, outline=st["border"], width=1.2)
    # 顶部色带
    rrect(x, CARD_Y, CARD_W, 58, 16, fill=st["tint"])
    d.rectangle([S(x), S(CARD_Y + 40), S(x + CARD_W), S(CARD_Y + 58)], fill=st["tint"])
    text(x + 22, CARD_Y + 18, st["sub"], 19, st["accent"], bold=True)
    text(x + 22, CARD_Y + 40, st["ui"], 12.5, C_TEXT3)

    # 条目
    y = CARD_Y + 84
    for title, note in st["items"]:
        d.ellipse([S(x + 24), S(y + 7), S(x + 30), S(y + 13)], fill=st["accent"])
        text(x + 40, y, title, 15.5, C_TEXT)
        y += 24
        if note:
            text(x + 40, y, note, 12.5, C_TEXT3)
            y += 21
        y += 15

    # 卡片之间的箭头（画在间隙中央，只在非末张）
    if i < 3:
        ax0, ax1, ay = x + CARD_W + 15, x + CARD_W + GAP - 15, CARD_Y + CARD_H / 2 - 6
        d.line([S(ax0), S(ay), S(ax1 - 9), S(ay)], fill="#c9c3e0", width=S(2))
        d.polygon(
            [(S(ax1), S(ay)), (S(ax1 - 11), S(ay - 6)), (S(ax1 - 11), S(ay + 6))],
            fill="#c9c3e0",
        )

# ══ 3. 今天要处理（串联）════════════════════════════════════════
BAR_Y, BAR_H = 668, 92
rrect(80, BAR_Y, 1440, BAR_H, 14, fill="#faf9ff", outline="#e0dbf6", width=1.4)
rrect(80, BAR_Y, 6, BAR_H, 3, fill="#7c6ff0")

text(112, BAR_Y + 20, "今天要处理", 20, "#4b3f8f", bold=True)
text(112, BAR_Y + 52, "不产生新内容，只回答「此刻该看什么」", 13.5, C_TEXT3)
text(112, BAR_Y + 70, "主屏文章列表栏顶部，可折叠；无待办时整条不渲染", 12.5, "#a9a4bd")

chips = [
    ("价格预警", "#c0453c", "#fdeceb"),
    ("财报披露", "#b0761f", "#fdf3e4"),
    ("判据到期", "#5b4fd0", "#f0eeff"),
    ("结构有偏", "#5b4fd0", "#f0eeff"),
    ("覆盖待定", "#237a5c", "#eaf6f1"),
]
cx = 1060
for cname, ccolor, cbg in chips:
    cw = len(cname) * 15 + 26
    rrect(cx, BAR_Y + 44, cw, 26, 13, fill=cbg)
    text(cx + cw / 2, BAR_Y + 57, cname, 13.5, ccolor, bold=True, anchor="mm")
    cx += cw + 8

text(1060, BAR_Y + 20, "五类来源，按「逾期 → 优先级 → 类别 → 目标 ID」排序", 12, C_TEXT3)

# 从四张卡片垂下的虚线
for x in xs:
    cx_mid = x + CARD_W / 2
    yy = CARD_Y + CARD_H + 8
    while yy < BAR_Y - 8:
        d.line([S(cx_mid), S(yy), S(cx_mid), S(min(yy + 7, BAR_Y - 8))],
               fill="#cfc9e4", width=S(1.6))
        yy += 14

# ══ 4. 贯穿全程 ═════════════════════════════════════════════════
d.line([S(80), S(806), S(1520), S(806)], fill=C_LINE, width=S(1))
text(80, 822, "贯穿全程", 14, "#6f6987", bold=True)

caps = [
    ("本地优先", "无账户、无云端，全部数据在本机 SQLite"),
    ("后台调度", "订阅按频率刷新 · 每日 3 点清理 · 每 5 分钟查预警"),
    ("快捷键与命令面板", "Ctrl/⌘ + K 检索 · 按 ? 查看全部键位"),
]
cw3 = 466
for i, (t, sub) in enumerate(caps):
    x = 80 + i * (cw3 + 21)
    rrect(x, 852, cw3, 70, 11, fill=C_WHITE, outline="#e6e3f0", width=1.2)
    text(x + 22, 870, t, 15, C_TEXT, bold=True)
    text(x + 22, 895, sub, 12.5, C_TEXT3)

# 页脚
text(1520, 966, "ReadFlow · docs/user-guide.md 第 1 节配图", 12, "#b0abc2", anchor="ra")

# ══ 5. 输出 ═════════════════════════════════════════════════════
out = img.resize((W, H), Image.LANCZOS)
dst = os.path.join(os.path.dirname(os.path.abspath(__file__)), "flow.png")
out.save(dst, "PNG", optimize=True)
print(f"saved: {dst} ({out.width}x{out.height})")
