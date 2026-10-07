#!/usr/bin/env bash
# 从品牌标记重新生成全部图标资产
#
# # 用途
# 品牌标记（琥珀渐变 + 白色流动箭头）改动后，用本脚本一键重出
# 应用图标全套尺寸、托盘图标、.ico 与 .icns，避免手工转换时尺寸/命名出错。
#
# # 依赖
# ImageMagick 7（`convert`）。不需要 rsvg / Inkscape / Python 三方库——
# 渐变走 `-sparse-color barycentric`，路径走 MVG `-draw`，都是内置能力。
#
# # 用法
#   ./regen-icons.sh          # 在 src-tauri/icons/ 下执行
#
# # 注意
# - ImageMagick 内置的 MSVG 渲染器**不支持 `url(#gradient)` 引用**（会输出全透明图），
#   因此本脚本完全不经过 SVG 渲染，而是用 MVG 原语直接画出同样的几何。
#   `logo.svg` 保留作为设计源文件（供设计工具查看 / 手工微调），不参与转换。
# - 托盘图标刻意**不含**容器底板：托盘可用高度仅 16–24px，
#   把"底板+渐变+标记"三层一起缩进去会糊成灰点，故只保留品牌标记本身。
set -euo pipefail

cd "$(dirname "$0")"

# ── 品牌色（须与 src/styles.css 的 --brand-from / --brand-to 保持一致）──
readonly BRAND_FROM="#f59e0b"
readonly BRAND_TO="#b45309"

# ── 几何常量（512 画布坐标，与 logo.svg 一致；渲染时按倍率缩放）──
# 流动弧线：从 (256,128) 左绕到 (256,384)
readonly ARC="M 512 256 C 352 256 256 368 256 512 C 256 656 352 768 512 768"
# chevron：弧线收笔的箭头
readonly CHEVRON="M 608 416 L 512 512 L 608 608"
readonly STROKE_W=80          # 512 画布上约 7.8%，缩到 16px 仍有约 1.25px 可见宽度
readonly CORNER=224           # 1024 画布圆角，约 22%
readonly MASTER=1024          # 主渲染尺寸，2 倍超采样后缩放各目标尺寸

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "▸ 渲染主图 ${MASTER}x${MASTER} …"
# 135° 线性渐变：sparse-color barycentric 直接在两点间插值，
# 不用 rotate——旋转会让画布角上出现旋转补边留下的空白。
convert -size ${MASTER}x${MASTER} xc: \
  -sparse-color barycentric "0,0 ${BRAND_FROM}  $((MASTER-1)),$((MASTER-1)) ${BRAND_TO}" \
  "$WORK/grad.png"

# 圆角方形遮罩
convert -size ${MASTER}x${MASTER} xc:none -fill white \
  -draw "roundrectangle 0,0 $((MASTER-1)),$((MASTER-1)) ${CORNER},${CORNER}" \
  "$WORK/mask.png"

# 底板 + 品牌标记
convert "$WORK/grad.png" "$WORK/mask.png" -alpha off -compose CopyOpacity -composite \
  -stroke white -strokewidth ${STROKE_W} -fill none \
  -draw "stroke-linecap round stroke-linejoin round path '${ARC}'" \
  -draw "stroke-linecap round stroke-linejoin round path '${CHEVRON}'" \
  "$WORK/master.png"

echo "▸ 输出应用图标 PNG …"
# 同时产出 Tauri 约定命名（tauri.conf.json 引用）与 icon- 前缀命名（历史兼容）
for s in 16 32 64 128 256 512; do
  convert "$WORK/master.png" -filter Lanczos -resize ${s}x${s} "icon-${s}x${s}.png"
  cp "icon-${s}x${s}.png" "${s}x${s}.png"
done
convert "$WORK/master.png" -filter Lanczos -resize 1024x1024 icon.png
convert "$WORK/master.png" icon-512x512@2x.png
cp icon-256x256.png icon-512x512.png      # 512 = 256 的 2x
cp icon-256x256.png 128x128@2x.png       # 256 = 128 的 2x

echo "▸ 输出 .ico（Windows，含 5 个尺寸）…"
convert icon-256x256.png icon-128x128.png icon-64x64.png \
        icon-32x32.png icon-16x16.png icon.ico

echo "▸ 输出 .icns（macOS，手工组装 PNG chunk）…"
# ImageMagick 不支持 ICNS 写出，但 ICNS 本身只是「PNG 的容器」：
#   'icns' + 总长度(BE u32) + 若干 chunk(type 4B + 长度BE u32 + PNG)
# 类型码低字节=宽、高字节=高（Retina 档位高/宽各 +0x100）
for s in 512 1024; do
  convert "$WORK/master.png" -filter Lanczos -resize ${s}x${s} "$WORK/i${s}.png"
done
cp icon-16x16.png "$WORK/i16.png"; cp icon-32x32.png "$WORK/i32.png"
cp icon-64x64.png "$WORK/i64.png"; cp icon-128x128.png "$WORK/i128.png"
cp icon-256x256.png "$WORK/i256.png"
python3 - "$WORK" <<'PY'
import struct, sys
w = sys.argv[1]
chunks = [
    (b'icp4', f'{w}/i16.png'),   (b'icp5', f'{w}/i32.png'),
    (b'icp6', f'{w}/i64.png'),   (b'ic07', f'{w}/i128.png'),
    (b'ic08', f'{w}/i256.png'),  (b'ic09', f'{w}/i512.png'),
    (b'ic10', f'{w}/i1024.png'),                      # Retina 5K
    (b'ic11', f'{w}/i64.png'),   (b'ic12', f'{w}/i128.png'),
    (b'ic13', f'{w}/i512.png'),  (b'ic14', f'{w}/i1024.png'),
]
body = b''.join(t + struct.pack('>I', len(d := open(p, 'rb').read()) + 8) + d
                for t, p in chunks)
open('icon.icns', 'wb').write(b'icns' + struct.pack('>I', len(body) + 8) + body)
PY

echo "▸ 输出托盘图标（单色标记，无容器）…"
# 独立几何：去掉圆角底板，只留标记，适配 16–24px 的托盘高度
convert -size 256x256 xc:none -stroke black -strokewidth 22 -fill none \
  -draw "stroke-linecap round stroke-linejoin round path 'M 150 57 C 106 57 79 88 79 128 C 79 168 106 199 150 199'" \
  -draw "stroke-linecap round stroke-linejoin round path 'M 176 101 L 150 128 L 176 155'" \
  "$WORK/tray.png"
convert "$WORK/tray.png" -filter Lanczos -resize 32x32   -define png:color-type=6 tray-icon.png
convert "$WORK/tray.png" -filter Lanczos -resize 64x64   -define png:color-type=6 tray-icon@2x.png
convert "$WORK/tray.png" -filter Lanczos -resize 16x16   -define png:color-type=6 tray-icon-sm.png

echo ""
echo "✓ 完成。已生成："
echo "  应用图标  16/32/64/128/256/512/1024 + icon.png"
echo "  平台封装  icon.ico (5 尺寸) / icon.icns (11 chunk)"
echo "  托盘图标  tray-icon.png (32) / @2x (64) / -sm (16)"
