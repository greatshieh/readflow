/**
 * 报告 Markdown 的展示层处理
 *
 * # 职责
 * 研究报告按 Obsidian 惯例渲染，落盘文件以 YAML frontmatter 开头（type / date /
 * period / events / entities / tags）。那段元数据是给 Obsidian 的 dataview 与标签
 * 系统消费的，磁盘上必须保留；但应用内查看器是等宽纯文本展示，frontmatter 会以
 * 一大段 `---` 与键值对的形式裸露在正文之前，反而把读者挡在报告外面。
 * 本模块只负责在**展示时**裁掉那一段，不修改磁盘上的文件。
 *
 * # 设计意图
 * - **纯函数、无副作用**：不依赖 Pinia、不触后端，可安全用于 computed。
 * - **保守匹配**：只有文件开头就是分隔符、且能找到配对结束行时才剥离。
 *   正文中间的水平分割线（以及以 `---` 作为内容开头的普通笔记）不会被误伤。
 */

/**
 * 剥掉报告开头的 YAML frontmatter
 *
 * 匹配规则与 Obsidian / YAML 前置元数据的约定一致：文件首行恰为 `---`，
 * 到下一个独占一行的 `---` 为止整段移除。不满足条件时原样返回，
 * 宁可让分隔符显示出来，也不要把正文当元数据删掉。
 *
 * @param markdown - 完整报告文本
 * @returns 去掉 frontmatter 后的正文（首尾空白已修剪）
 */
export function stripFrontmatter(markdown: string): string {
  const lines = markdown.split('\n')
  if (lines.length === 0 || lines[0].trim() !== '---') return markdown

  for (let i = 1; i < lines.length; i++) {
    if (lines[i].trim() === '---') {
      return lines.slice(i + 1).join('\n').trim()
    }
  }

  // 只有开头没有结尾：不是 frontmatter，原样返回
  return markdown
}
