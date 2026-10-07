/**
 * 文章追问问答 Store（Pinia）
 *
 * # 职责
 * 管理"就当前文章与 AI 多轮对话"的全部前端状态：按文章分桶的会话记录、流式回答的
 * 实时累积、提问 / 停止 / 清空 / 导出。对外暴露 `useArticleChatStore()`，
 * 由 `components/ArticleChat.vue` 消费。
 *
 * # 设计意图
 * - **会话只在内存**：不落库、不建表。追问是"私有草稿"，是否留痕交给用户——
 *   需要时就点「导出」，导出走后端 `save_text_file`（系统「另存为」对话框）。
 *   如此既不必为一次性草稿引入 schema 变更，也不会在库里堆积无人清理的对话。
 * - **按文章分桶**：切走文章再切回来，上一轮对话仍在（应用本次运行期间有效，
 *   关掉应用即丢失）。
 * - **流式状态放在模块作用域**：`ai-stream` 监听器只注册一次，若闭包捕获的是某个
 *   store 实例的 ref，store 被重建（HMR、多实例）之后增量就会写进没人渲染的旧 ref，
 *   界面再也不动——这与 `stores/articles.ts` 的处理完全一致。
 * - **与摘要 / 翻译互不干扰**：本 store 只消费 `kind === 'chat'` 的事件并额外比对
 *   `requestId`；`articles.ts` 的监听器只比对它自己的 `requestId`，两者的标识不可能
 *   相等，因此两条链路可以共存而不会把增量画到对方的区域里。
 *
 * # Tauri invoke 参数约定
 * 前端传参一律使用 **camelCase**（`articleId`、`providerId`、`requestId`），
 * Rust 端以 snake_case 接收，Tauri 自动完成转换。
 */

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { AiStreamPayload, ChatMessage } from '@/types'
import { newRequestId } from '@/utils/requestId'
import { buildChatMarkdown } from '@/utils/chatMarkdown'

/** 单次回带的历史消息上限（与后端 `ARTICLE_CHAT_MAX_HISTORY` 保持一致） */
const HISTORY_LIMIT = 20

/** 流式累积的回答文本（模块作用域，理由见文件头注释） */
const streamText = ref('')
/** 正在生成回答的文章 ID；null 表示当前没有在途回答 */
const streamArticleId = ref<number | null>(null)

/**
 * 在途请求的标识（非响应式）
 *
 * 同时承担两个作用：过滤迟到事件、判断"本次请求是否已被停止"。`stop()` 会把它置空，
 * 于是此后所有增量与 `invoke` 的最终结果都会被丢弃。
 */
let activeRequestId: string | null = null

/** 监听是否已注册（模块级，避免 store 重建时重复订阅） */
let listenerRegistered = false

// 订阅后端逐批推出的 AI 增量。监听失败（非 Tauri 环境等）只记日志：
// 拿不到增量最多是少了逐字效果，`invoke` 的返回值里仍有完整回答。
if (!listenerRegistered) {
  listenerRegistered = true
  import('@tauri-apps/api/event')
    .then(({ listen }) => {
      listen<AiStreamPayload>('ai-stream', (event) => {
        const p = event.payload
        // 只管自己的事件：kind 区分功能，requestId 丢弃上一轮迟到的增量，
        // articleId 兜住"用户已经切到别的文章"的情形
        if (p.kind !== 'chat') return
        if (p.requestId !== activeRequestId) return
        if (p.articleId !== streamArticleId.value) return
        streamText.value += p.delta
      })
    })
    .catch((e) => console.warn('订阅 AI 流式事件失败:', e))
}

/**
 * 把标题转成可安全落盘的文件名
 *
 * 只剔除各平台公认的非法字符并限制长度：RSS 标题里出现 `:` `/` `?` 很常见，
 * 原样拿去当默认文件名会让系统对话框直接报错或静默截断。
 *
 * @param title - 文章标题
 * @returns 可用的文件名片段（全部字符都非法时退化为「文章」）
 */
function safeFileStem(title: string): string {
  const cleaned = title.replace(/[\\/:*?"<>|]/g, '_').trim()
  return cleaned.slice(0, 60) || '文章'
}

export const useArticleChatStore = defineStore('articleChat', () => {
  /** 会话记录，按文章 ID 分桶（内存，刻意不落库） */
  const sessions = ref<Record<number, ChatMessage[]>>({})

  /** 最近一次失败的提示（下一次提问时清空） */
  const error = ref('')

  /** 是否已有在途回答（用于禁用输入与切换发送/停止按钮） */
  const busy = ref(false)

  /**
   * 取某篇文章的会话记录
   *
   * @param articleId - 文章 ID
   * @returns 消息数组；该文章还没有对话时返回空数组（**不**写入 store，
   *          避免读一次就凭空产生一个空桶）
   */
  function messages(articleId: number): ChatMessage[] {
    return sessions.value[articleId] ?? []
  }

  /**
   * 取（不存在则创建）某篇文章的会话数组
   *
   * @param articleId - 文章 ID
   * @returns 可直接 push 的响应式数组引用
   */
  function ensure(articleId: number): ChatMessage[] {
    const existing = sessions.value[articleId]
    if (existing) return existing
    const created: ChatMessage[] = []
    sessions.value[articleId] = created
    return created
  }

  /**
   * 就当前文章提问一次，并等待流式回答
   *
   * 期间界面通过模块级的 `streamText` / `streamArticleId` 渲染逐字效果；
   * 回答完成后才把整段文本写进会话记录。
   *
   * @param articleId - 文章 ID
   * @param text - 用户提问（首尾空白会被裁掉；为空则忽略）
   * @param providerId - AI 提供商 ID（对应 Rust 端 `provider_id`），默认 'agnes'
   * @returns 无返回值；成功后会话追加一条 assistant 消息，失败则追加一条带
   *          `error` 标记的消息并把原因写入 `error`
   */
  async function send(articleId: number, text: string, providerId = 'agnes'): Promise<void> {
    const content = text.trim()
    // 已有在途回答时不接受新提问：后端是全流式单通道，并发提问只会让两段增量交错
    if (!content || busy.value) return

    error.value = ''
    const list = ensure(articleId)
    list.push({ role: 'user', content })

    // 回带历史：剔除失败消息（它们不是模型的真实输出，回带会污染上下文），
    // 并只保留最近 N 条，与后端上限一致
    const history = list
      .filter((m) => !m.error)
      .slice(-HISTORY_LIMIT)
      .map((m) => ({ role: m.role, content: m.content }))

    const requestId = newRequestId()
    activeRequestId = requestId
    streamText.value = ''
    streamArticleId.value = articleId
    busy.value = true

    try {
      const answer = await invoke<string>('ai_article_chat', {
        articleId,
        history,
        providerId,
        requestId
      })
      // 已被 stop() / clear() 丢弃：不写回会话，界面只保留停止时那一段
      if (activeRequestId !== requestId) return
      list.push({ role: 'assistant', content: answer })
    } catch (e) {
      if (activeRequestId !== requestId) return
      const msg = e instanceof Error ? e.message : String(e)
      error.value = msg
      // 失败也留在会话里：用户才能看见自己问了什么并据此重试
      list.push({ role: 'assistant', content: msg, error: true })
    } finally {
      // 仅当仍是本次请求时才收尾；被 stop() / clear() 接管的情形已由它们自行清理
      if (activeRequestId === requestId) {
        activeRequestId = null
        streamArticleId.value = null
        streamText.value = ''
        busy.value = false
      }
    }
  }

  /**
   * 停止当前回答
   *
   * 两级停止：
   * 1. **界面级（立即）**：断开归属——此后所有增量与 invoke 的最终结果都会被丢弃，
   *    保留已经生成的部分并标记 `stopped`（半截回答往往已包含有效信息，但要显式标注，
   *    免得被误当成完整回答）。
   * 2. **后端级（真取消）**：invoke `ai_cancel` 在 Rust 侧登记取消标记，
   *    SSE 循环逐块检查到即中断 HTTP 请求——服务端随即停止生成，省下未完成的 token。
   *    取消导致的 invoke reject（"已取消本次生成"）会被 send() 的归属判断吞掉，
   *    不会显示成错误。
   *
   * @returns 无返回值；有在途回答且已产生文本时，向该文章会话追加一条 `stopped` 消息
   */
  function stop(): void {
    const target = streamArticleId.value
    if (target === null) return

    const partial = streamText.value
    const rid = activeRequestId
    // 先断开归属：此后所有增量与 invoke 的最终结果都会被丢弃
    activeRequestId = null
    streamArticleId.value = null
    streamText.value = ''
    busy.value = false

    // 真取消：通知后端中断请求。fire-and-forget——失败只影响省 token，
    // 不影响界面已完成的界面级停止，故吞掉错误
    if (rid !== null) {
      invoke('ai_cancel', { requestId: rid }).catch(() => {})
    }

    if (partial.trim()) {
      ensure(target).push({ role: 'assistant', content: partial, stopped: true })
    }
  }

  /**
   * 清空某篇文章的对话（不可撤销）
   *
   * @param articleId - 文章 ID
   * @returns 无返回值；若清空的正是正在生成的那篇，同时中断流式渲染
   */
  function clear(articleId: number): void {
    // 换一个新数组而不是就地清空：正在 await 的 send() 仍持有旧数组引用，
    // 换引用能让它即便漏判也不会把旧内容写回"已清空"的会话
    sessions.value[articleId] = []
    if (streamArticleId.value === articleId) {
      // 清空的正是正在生成的那篇：同样走真取消，理由与 stop() 相同
      if (activeRequestId !== null) {
        invoke('ai_cancel', { requestId: activeRequestId }).catch(() => {})
      }
      activeRequestId = null
      streamArticleId.value = null
      streamText.value = ''
      busy.value = false
    }
  }

  /**
   * 把会话导出为 Markdown 文件
   *
   * 文件读写一律在 Rust 侧完成（系统「另存为」对话框），前端只负责拼文本，
   * 因此不存在"WebView 里触发下载被静默拦下"的隐患。
   *
   * @param articleId - 文章 ID
   * @param articleTitle - 文章标题（写入文件头，并作为默认文件名）
   * @returns 已写入的绝对路径；用户取消对话框时为 `null`；会话为空时也为 `null`
   * @throws 后端写入失败时 reject，由调用方提示
   */
  async function exportMarkdown(articleId: number, articleTitle: string): Promise<string | null> {
    const list = messages(articleId)
    if (list.length === 0) return null

    const content = buildChatMarkdown(articleTitle, list, new Date())
    return invoke<string | null>('save_text_file', {
      defaultName: `${safeFileStem(articleTitle)}-追问.md`,
      filterName: 'Markdown 文件',
      extensions: ['md'],
      content
    })
  }

  return {
    sessions,
    error,
    busy,
    streamText,
    streamArticleId,
    messages,
    send,
    stop,
    clear,
    exportMarkdown
  }
})
