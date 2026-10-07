/**
 * 文章追问问答的 Markdown 导出
 *
 * # 职责
 * 把内存里的一次问答会话渲染成可直接存档、或交给 Obsidian 等工具的 Markdown 文本。
 *
 * # 设计意图
 * - **纯函数、无副作用**：不依赖 Pinia、不触后端、不读系统时间（时间由调用方传入），
 *   因此导出内容的形态可以被单独推理与校验。
 * - **带出处元信息**：记录来源文章标题与导出时间——对话一旦脱离应用，
 *   没有这段头部就无从判断它在讨论什么。
 * - **用引用块区分角色**：`>` 在主流 Markdown 渲染器里支持稳定，
 *   比 `**问：**` 这类自定义前缀更不容易与正文里的加粗、列表混淆。
 */

import type { ChatMessage } from '@/types'

/**
 * 生成导出用的 Markdown 文本
 *
 * @param articleTitle - 来源文章标题（写入一级标题与元信息）
 * @param messages - 会话消息，按时间顺序
 * @param exportedAt - 导出时间；由调用方传入而非在此取当前时间，便于测试
 * @returns 完整的 Markdown 文本（以换行结尾）
 */
export function buildChatMarkdown(
  articleTitle: string,
  messages: ChatMessage[],
  exportedAt: Date
): string {
  const lines: string[] = [
    `# ${articleTitle} — 追问记录`,
    '',
    `> 导出时间：${exportedAt.toLocaleString()}`,
    `> 共 ${messages.length} 条消息`,
    ''
  ]

  for (const m of messages) {
    // 多行内容必须逐行加前缀：只给首行加 `>` 会让后续段落掉出引用块
    const body = m.content
      .split('\n')
      .map((line) => `> ${line}`)
      .join('\n')
    lines.push(`**${m.role === 'user' ? '我' : 'AI'}**${m.error ? '（该次回答失败）' : ''}`)
    lines.push('')
    lines.push(body)
    lines.push('')
  }

  return lines.join('\n').trimEnd() + '\n'
}
