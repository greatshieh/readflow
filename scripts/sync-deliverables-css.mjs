/**
 * 把构建产物 CSS 同步成交付页的固定入口 `deliverables/app.css`
 *
 * # 为什么需要它
 * `dist/` 被 gitignore，而 `deliverables/*.html` 要引用构建产物里的样式，
 * 于是引用只能写成带内容哈希的路径（`../dist/assets/index-DcZ502-z.css`）。
 * 哈希每次构建都会变——哪怕只改了脚本、CSS 一个字节没变——
 * 结果就是"每次构建都要回头改 5 个 HTML 里的 8 处引用"。
 * 漏改的后果是整页 404、页面失色，而不是一处小瑕疵。
 *
 * 改成固定文件名后，HTML 里的引用永不需改动；本脚本挂在上面的
 * `build:frontend` 尾部自动覆盖，忘了跑也只是"样式旧一版"，
 * 不会让页面打不开。
 *
 * # 参数与返回
 * 无参数，无返回值（CLI 脚本）。成功时打印源文件与大小到 stdout。
 *
 * # 错误
 * `dist/assets/` 不存在或其中没有 `index-*.css` 时，打印原因并 `exit(1)`。
 * **刻意不静默成功**：构建没产物却让交付页"看起来同步过了"，
 * 会把手上的问题推迟到别人打开页面时才发现。
 */

import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const distAssets = 'dist/assets'
const target = join('deliverables', 'app.css')

if (!existsSync(distAssets)) {
  console.error(`[sync-css] 找不到 ${distAssets}/，请先跑 vite build`)
  process.exit(1)
}

// 正常只有一个 index-*.css；若残留多个旧产物，按修改时间取最新的
// （`vite build` 会清空 outDir，出现多个说明有别的进程往这里写过）
const candidates = readdirSync(distAssets)
  .filter((f) => f.startsWith('index-') && f.endsWith('.css'))
  .map((f) => ({ f, mtime: statSync(join(distAssets, f)).mtimeMs }))
  .sort((a, b) => b.mtime - a.mtime)

if (candidates.length === 0) {
  console.error(`[sync-css] ${distAssets}/ 里没有 index-*.css，构建可能失败了`)
  process.exit(1)
}
if (candidates.length > 1) {
  console.warn(`[sync-css] 发现 ${candidates.length} 个候选，取最新的 ${candidates[0].f}`)
}

const source = join(distAssets, candidates[0].f)

// 读一次再比对：内容相同就不落盘，避免无意义的 mtime 抖动。
// 刻意不用 copyFileSync——它在受限环境下会被拦（EPERM: copyfile），
// 而"读一份写一份"在任何环境都能跑通，代价只是多一瞬内存。
const bytes = readFileSync(source)
if (existsSync(target) && readFileSync(target).equals(bytes)) {
  console.log(`[sync-css] ${target} 已是最新（${candidates[0].f}）`)
} else {
  writeFileSync(target, bytes)
  // 用 KiB 而不是照抄 vite 报的 "314.10 kB"：那个数是字符串 `length` ÷ 1000，
  // 不是真实字节。本项目 CSS 里中文注释很多，一个汉字 length 记 1、UTF-8 占 3 字节，
  // 于是 vite 的输出会系统性低估（当前实测差 82 kB）。
  console.log(`[sync-css] ${candidates[0].f} → ${target}（${(bytes.length / 1024).toFixed(2)} KiB）`)
}
