/**
 * 文章列表缩略图（纯函数工具）
 *
 * # 职责
 * 从文章的 HTML 中提取首张 http(s) 图片作为列表缩略图，并缓存提取结果。
 *
 * # 为什么缓存放在模块级
 * 列表滚动时渲染函数会反复求值，每次都跑一遍正文 HTML 正则纯属浪费；缓存以文章 ID 为键，
 * 天然跨组件实例共享（列表项组件是每行一个实例，缓存若放在组件里就退化成"每行各存一份"）。
 * 正文只在重新抓取时才变，因此只需在「刷新 / 换排序 / 换范围」这些会重新取数的时机
 * 调用 {@link clearThumbnailCache} 清空。
 */

import type { Article } from '@/types'

/** 文章 ID → 缩略图 URL（空串表示该文无图，同样入缓存以免反复匹配） */
const cache = new Map<number, string>()

/**
 * 取文章在列表中使用的缩略图
 *
 * 优先取正文 `content`，没有正文时退回 `summary`——不少源的 `summary` 里带首图，
 * 而 `content` 为空时列表会显得光秃秃。
 *
 * @param article - 目标文章
 * @returns 可用的图片 URL；无图时返回空串（调用方据此不渲染缩略图位）
 */
export function getThumbnail(article: Article): string {
  const cached = cache.get(article.id)
  if (cached !== undefined) return cached
  const html = article.content || article.summary
  const m = html?.match(/<img[^>]+src=["'](https?:\/\/[^"']+)["']/i)
  const thumb = m ? m[1] : ''
  cache.set(article.id, thumb)
  return thumb
}

/**
 * 清空缩略图缓存
 *
 * 在文章列表被重新拉取前调用（刷新当前源 / 切换排序 / 切换范围）：
 * 同一篇文章的正文可能已更新，留着旧缩略图会与新内容不一致。
 *
 * @returns 无返回值
 */
export function clearThumbnailCache(): void {
  cache.clear()
}
