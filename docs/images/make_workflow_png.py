#!/usr/bin/env python3
"""生成 ReadFlow「一页纸工作流」时间轴 PNG。

与 make_flow_png.py 同一套画法（Pillow 直绘，不依赖 SVG 渲染器）。
纵向时间轴：五个时段各一段，左列是时间，右列是动作与要点。

为什么纵向而不横向：内容是从早到晚的顺序推进，纵向时间轴能让
「几点做什么」一眼对齐；横向排五行会把每条压得很窄，要点写不下。

用法：python make_workflow_png.py
"""

import os
from PIL import Image, ImageDraw, ImageFont

SCALE = 2
W, H = 1240, 1800   # H 只是画布上限，最终按实际内容裁剪（见文件末尾）
BG = "#fbfbfd"

C_TEXT = "#2b2740"
C_TEXT2 = "#5c5675"
C_TEXT3 = "#8b87a3"
C_LINE = "#e6e3f0"
C_WHITE = "#ffffff"

# ── 五个时段 ────────────────────────────────────────────────────────
# steps: [(条目标题, 补充说明 or None), ...]
SLOTS = [
    {
        "time": "早上",
        "clock": "打开就做",
        "stage": "看",
        "accent": "#4a7be0", "tint": "#eef1ff",
        "steps": [
            ("看「今天要处理」，逐条点「打开」", "跳转不等于已处理，条目会留着，直到你真的做完"),
        ],
    },
    {
        "time": "上午",
        "clock": "读到判断",
        "stage": "读",
        "accent": "#7c6ff0", "tint": "#f2efff",
        "steps": [
            ("读文章 · 选中文字高亮 · 追问 AI · 看摘要", None),
        ],
    },
    {
        "time": "午后",
        "clock": "形成沉淀",
        "stage": "沉淀",
        "accent": "#2e9c76", "tint": "#e8f7f1",
        "steps": [
            ("研究事件悬浮按钮 → 抽取 → 展开看价格反应", None),
            ("挂到论点（反驳优先）", "反驳置顶是故意的，先读「我可能错在哪」"),
            ("改置信，写一句理由；必要时切状态", "理由可留空，但半年后你想看的正是它"),
        ],
    },
    {
        "time": "盘中",
        "clock": "盯价格",
        "stage": "盯",
        "accent": "#c0453c", "tint": "#fdeceb",
        "steps": [
            ("实体仪表盘 →「同步行情」", "行情不会自己来，要先点一次"),
            ("设一条价格预警 → 点「立即检查」", "别等到明天才发现规则从没跑过"),
        ],
    },
    {
        "time": "晚上",
        "clock": "收工",
        "stage": "复盘",
        "accent": "#b0761f", "tint": "#fdf3e4",
        "steps": [
            ("按 W 进复盘 → 处理「该复查了」", None),
            ("点「结束本次复盘」", "按 Esc 只是离开页面，不算结束"),
        ],
    },
    {
        "time": "周末",
        "clock": "写出去",
        "stage": "输出",
        "accent": "#6f6987", "tint": "#f2f1f7",
        "steps": [
            ("生成周期报告 → 读 → 导出 md / HTML", None),
            ("检查覆盖清单里每个标的的「退出条件」有没有写", "没写会显示黄色警告"),
            ("组装一页 → 粘进 Obsidian 写正文", None),
        ],
    },
]

FONT_CANDS = [
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
]
FONT_BOLD_CANDS = [
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    "/usr/share/fonts/opentype/noto/NotoSerifCJK-Bold.ttc",
]


def pick(cands):
    for p in cands:
        if os.path.exists(p):
            return p
    raise RuntimeError("找不到中文字体，请安装 fonts-noto-cjk")


F_REG, F_BOLD = pick(FONT_CANDS), pick(FONT_BOLD_CANDS)


def S(v):
    return int(round(v * SCALE))


def font(size, bold=False):
    return ImageFont.truetype(F_BOLD if bold else F_REG, int(size * SCALE))


img = Image.new("RGB", (S(W), S(H)), BG)
d = ImageDraw.Draw(img)


def text(x, y, s, size=16, color=C_TEXT, bold=False, anchor="la"):
    d.text((S(x), S(y)), s, font=font(size, bold), fill=color, anchor=anchor)


def rrect(x, y, w, h, r, fill=None, outline=None, width=1):
    d.rounded_rectangle(
        [S(x), S(y), S(x + w), S(y + h)], radius=S(r),
        fill=fill, outline=outline, width=max(1, S(width)),
    )


# ══ 标题 ══════════════════════════════════════════════════════════
text(80, 50, "一页纸工作流", 38, C_TEXT, bold=True)
text(80, 102, "从早到晚，一条信息怎么被推到能写进报告", 18, C_TEXT3)
d.line([S(80), S(140), S(1160), S(140)], fill=C_LINE, width=S(1))

# ══ 时间轴 ═══════════════════════════════════════════════════════
TL_X = 152          # 时间柱的横坐标
CARD_X = 208        # 卡片左边界
CARD_W = 952        # 卡片宽度
TOP = 190
CARD_GAP = 26

y = TOP
slot_tops = []

for i, slot in enumerate(SLOTS):
    # 每条步骤占 46（标题）+ 21（有说明时）；卡片上下留白 56 + 18
    n_steps = len(slot["steps"])
    n_notes = sum(1 for _, note in slot["steps"] if note)
    card_h = 56 + 18 + n_steps * 46 + n_notes * 21 - 12
    slot_tops.append((y, card_h))

    # 左侧：时段名 + 副标（右对齐到时间柱左侧）
    text(TL_X - 34, y + 4, slot["time"], 21, C_TEXT, bold=True, anchor="ra")
    text(TL_X - 34, y + 34, slot["clock"], 12.5, C_TEXT3, anchor="ra")

    # 时间柱上的节点：圆点（白色内芯，稍后统一重画一遍压在线上）
    d.ellipse([S(TL_X - 9), S(y + 8), S(TL_X + 9), S(y + 26)], fill=slot["accent"])
    d.ellipse([S(TL_X - 3.5), S(y + 13.5), S(TL_X + 3.5), S(y + 20.5)], fill=C_WHITE)

    # 卡片
    rrect(CARD_X, y, CARD_W, card_h, 14, fill=C_WHITE, outline=slot["tint"], width=1.6)
    # 左侧色轨
    rrect(CARD_X, y, 5, card_h, 2.5, fill=slot["accent"])

    # 阶段标签
    tag = slot["stage"]
    tw = len(tag) * 15 + 24
    rrect(CARD_X + 24, y + 14, tw, 26, 13, fill=slot["tint"])
    text(CARD_X + 24 + tw / 2, y + 27, tag, 13.5, slot["accent"], bold=True, anchor="mm")

    # 步骤
    sy = y + 56
    for title, note in slot["steps"]:
        d.ellipse([S(CARD_X + 30), S(sy + 6), S(CARD_X + 36), S(sy + 12)], fill=slot["accent"])
        text(CARD_X + 46, sy, title, 15.5, C_TEXT)
        sy += 26
        if note:
            text(CARD_X + 46, sy, note, 12.5, C_TEXT3)
            sy += 21
        sy += 20

    y += card_h + CARD_GAP

# 时间柱竖线：从第一个节点连到最后一个节点
first_dot = slot_tops[0][0] + 17
last_top, last_h = slot_tops[-1]
last_dot = last_top + 17
d.line([S(TL_X), S(first_dot), S(TL_X), S(last_dot)], fill="#ddd8ec", width=S(2.5))

# 竖线要压在圆点下面，重画一遍圆点（顺序修正）
for i, slot in enumerate(SLOTS):
    ty = slot_tops[i][0]
    d.ellipse([S(TL_X - 9), S(ty + 8), S(TL_X + 9), S(ty + 26)], fill=slot["accent"])
    d.ellipse([S(TL_X - 3.5), S(ty + 13.5), S(TL_X + 3.5), S(ty + 20.5)], fill=C_WHITE)

# ══ 底部提示 ═════════════════════════════════════════════════════
FOOT_Y = y + 20
rrect(80, FOOT_Y, 1080, 66, 12, fill="#faf9ff", outline="#e0dbf6", width=1.2)
rrect(80, FOOT_Y, 5, 66, 2.5, fill="#7c6ff0")
text(108, FOOT_Y + 13, "这份节奏不是必须遵守的", 14.5, "#4b3f8f", bold=True)
text(
    108, FOOT_Y + 37,
    "早上那条「今天要处理」本身就是把后面几步里到期的东西挑了出来 —— 没事可做时它整条不显示，那就直接去读。",
    13, C_TEXT3,
)

text(1160, FOOT_Y + 96, "ReadFlow · docs/user-guide.md 附录配图", 12, "#b0abc2", anchor="ra")

# ══ 输出 ═════════════════════════════════════════════════════════
# 高度按实际内容收尾（FOOT 底 + 页脚留白），不能写死——写死就会裁掉最后一屏
final_h = int(FOOT_Y + 66 + 62)   # 66 是提示框高，后面留足页脚位置
out = img.crop((0, 0, S(W), S(final_h))).resize((W, final_h), Image.LANCZOS)
dst = os.path.join(os.path.dirname(os.path.abspath(__file__)), "workflow.png")
out.save(dst, "PNG", optimize=True)
print(f"saved: {dst} ({out.width}x{out.height})")
