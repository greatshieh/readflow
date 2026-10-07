/**
 * 类型定义
 *
 * 所有类型与 Rust 后端 db.rs 中的数据模型保持对应。
 */

/** 订阅源 */
export interface Feed {
  /** 唯一标识符 */
  id: number
  /** 订阅源名称 */
  name: string
  /** RSS/Atom URL */
  url: string
  /** 图标 URL */
  icon?: string
  /** 所属文件夹 ID（0 表示未分类） */
  folder_id: number
  /** 未读文章数 */
  unread_count: number
  /** 创建时间 */
  created_at: string
  /** 更新时间 */
  updated_at: string
}

/** 文章内容 */
export interface Article {
  /** 唯一标识符 */
  id: number
  /** 所属订阅源 ID */
  feed_id: number
  /** 文章标题 */
  title: string
  /** 原文链接 */
  link: string
  /** 摘要 */
  summary: string
  /** 正文内容（HTML） */
  content: string
  /** 作者 */
  author?: string
  /** 发布时间 */
  published_at: string
  /** 是否已读 */
  is_read: boolean
  /**
   * 阅读进度：正文滚动位置占「可滚高度」的比例，0.0–1.0
   *
   * 后端 `Article::set_progress` 负责 clamp，前端只负责上报与恢复。
   * 恢复到 `scrollTop` 前必须先等正文渲染完，否则 `scrollHeight` 偏小会把位置算歪。
   */
  read_progress: number
  /** 是否已收藏 */
  is_bookmarked: boolean
  /** 是否有 AI 摘要 */
  has_ai_summary: boolean
  /** 是否有翻译 */
  has_ai_translation: boolean
  /** 智能评分（基于实体匹配度，0-100） */
  entity_score: number
  /**
   * 研究提取的**尝试**时间（null / 缺省 = 从未提取过）
   *
   * 记录的是"尝试过"而非"抽到了事件"，因此它是阅读区判断
   * 「尚未提取」与「已提取但本文无事件」的唯一依据。
   */
  ai_extracted_at?: string | null
  /** AI 摘要内容 */
  ai_summary?: string
  /** 翻译内容 */
  ai_translation?: string
  /** 使用的 AI 模型 */
  ai_model?: string
  /**
   * 文章标签（多对多）
   *
   * 由后端在返回前批量挂载（`Tag::for_articles`），未挂载时为空数组。
   * 不是 `articles` 表的列，前端只读渲染 + 通过 `article_tags_set` 整组覆盖。
   */
  tags: Tag[]
  /** 创建时间 */
  created_at: string
}

/**
 * 标签
 *
 * 用户侧的整理维度：手工打标或由过滤规则（`action = "tag"`）自动打标。
 * 与「关注实体」（{@link Entity}）不同——实体服务于自动评分与研究提取，
 * 标签是给人看的分类，两者不要混用。
 */
export interface Tag {
  /** 唯一标识符 */
  id: number
  /** 标签名（ASCII 大小写不敏感去重） */
  name: string
  /** 标签颜色（空串表示使用界面默认色） */
  color: string
  /** 该标签下的文章数（由后端 LEFT JOIN 聚合得出） */
  article_count: number
  /** 创建时间 */
  created_at: string
}

/** 应用设置项 */
export interface Setting {
  /** 设置键 */
  key: string
  /** 设置值 */
  value: string
}

/** 全库数据统计（对齐后端 `Stats`） */
export interface Stats {
  /** 订阅源总数 */
  feeds: number
  /** 文章总数 */
  articles: number
  /** 已读文章数 */
  read: number
  /** 未读文章数 */
  unread: number
  /** 收藏文章数 */
  bookmarks: number
  /** 今日已读文章数 */
  read_today: number
}

/** 文件夹（订阅源分组） */
export interface Folder {
  /** 文件夹 ID */
  id: number
  /** 文件夹名称 */
  name: string
  /** 侧边栏显示顺序 */
  position: number
}

/** 过滤规则（规则引擎） */
export interface FilterRule {
  /** 规则 ID */
  id: number
  /** 规则名称 */
  name: string
  /** 是否启用 */
  enabled: boolean
  /** 作用范围：all / feed / folder */
  scope: string
  /** 作用范围限定的 ID（feed 时为 feed_id，folder 时为 folder_id） */
  scope_id: number
  /** 匹配字段：title / content / author / feed */
  field: string
  /** 匹配方式：contains / not_contains / equals / not_equals */
  op: string
  /** 匹配值 */
  value: string
  /** 命中动作：mark_read / star / hide / tag */
  action: string
  /**
   * 动作参数
   *
   * 仅 `action === 'tag'`（自动打标签）时有意义，存标签名；标签不存在时后端会自动创建。
   * 其余动作恒为空串。标签被删除后，引用它的规则会被后端把本字段清空（规则保留）。
   */
  action_arg: string
  /** 优先级（大的先求值） */
  priority: number
  /** 创建时间 */
  created_at: string
}

/** 正文高亮片段 */
export interface Highlight {
  /** 高亮 ID */
  id: number
  /** 所属文章 ID */
  article_id: number
  /** 高亮原文 */
  text: string
  /** 批注 */
  note: string
  /** 高亮颜色 */
  color: string
  /** 文本起点偏移 */
  start_offset: number
  /** 文本终点偏移 */
  end_offset: number
  /** 创建时间 */
  created_at: string
}

/**
 * 智能摘要里的一个实体分组
 *
 * 后端 `high_score_articles` 的返回元素：命中判定（实体名出现在标题或摘要中）
 * 与 `entity_score` 的计算口径完全一致，因此前端只需渲染，不必再猜命中了谁。
 */
export interface EntityArticleGroup {
  /** 实体名（分组标题） */
  entity: string
  /** 实体类型：company / person / product / industry / other */
  entity_type: string
  /** 该实体命中的高分文章（评分倒序） */
  articles: Article[]
}

/** 关注实体（用户关心的公司、人物、产品等） */
export interface Entity {
  /** 唯一标识符 */
  id: number
  /** 实体名称（如 "腾讯"、"GOOG"、"OpenAI"） */
  name: string
  /** 实体类型：company / person / product / industry / other */
  entity_type: string
  /** 别称列表（JSON 数组字符串，如 `["字节","ByteDance"]`），用于实体归一化 */
  aliases: string
  /** 是否启用 */
  enabled: boolean
  /** 创建时间 */
  created_at: string
}

/**
 * 实体关系图的节点
 *
 * 后端 `entity_graph` 的返回元素。`article_count` 是**所选时间窗内**
 * 标题或摘要命中该实体名的文章数，前端据此决定节点半径。
 */
export interface GraphNode {
  /** 实体 ID（与 `GraphEdge` 的 source/target 对应） */
  id: number
  /** 实体名称（节点标签） */
  name: string
  /** 实体类型：company / person / product / industry / other（决定节点配色） */
  entity_type: string
  /** 命中文章数（时间窗内），用于决定节点大小 */
  article_count: number
}

/**
 * 实体关系图的边
 *
 * 无向边，后端保证 `source < target`，因此前端可直接以 `${source}-${target}` 作键。
 */
export interface GraphEdge {
  /** 端点实体 ID（较小者） */
  source: number
  /** 端点实体 ID（较大者） */
  target: number
  /** 共现文章数（决定边的粗细） */
  weight: number
}

/** 实体共现图（一次请求返回完整图，前端不再二次查询） */
export interface EntityGraph {
  /** 节点（只含出现在边上的实体；孤立实体不返回） */
  nodes: GraphNode[]
  /** 边（按权重降序） */
  edges: GraphEdge[]
  /** 参与扫描的文章数，用于提示「已分析 N 篇」 */
  scanned_articles: number
  /** 边是否因超出上限被截断 */
  truncated: boolean
}

/** 研究事件（从文章中结构化提取的行业/公司动态） */
export interface ResearchEvent {
  /** 唯一标识符 */
  id: number
  /** 来源文章 ID */
  article_id: number
  /** 归一化后的实体 ID */
  entity_id: number
  /** 事件类型：product / executive / ma / regulatory / strategy / competition / industry_signal */
  event_type: string
  /** 事件发生日期（ISO 8601 字符串；空串表示未知） */
  event_date: string
  /** 客观事实一句话（非概括、非评价） */
  fact: string
  /** 原文片段（证据，用于溯源与反幻觉校验） */
  evidence: string
  /** 抽取所用模型 */
  source_model: string
  /** 入库时间 */
  created_at: string
}

/** 带上下文的研究事件行（JOIN 实体名与文章标题，时间线展示用） */
export interface ResearchEventWithContext extends ResearchEvent {
  /** 实体规范名（如 "字节跳动"） */
  entity_name: string
  /** 来源文章标题（文章可能已被保留策略清理，此时为 null） */
  article_title: string | null
  /** 来源文章链接（同上） */
  article_link: string | null
}

/** 自动化任务（任务即数据：定时提取 / 定时报告等） */
export interface AutomationTask {
  /** 唯一标识符 */
  id: number
  /** 任务名称（如"每日研究报告"） */
  name: string
  /** 任务类型：batch_extract（批量提取）/ daily_report（日报）/ weekly_report（周报） */
  task_type: string
  /** cron 表达式（tokio-cron-scheduler 格式，如 "0 30 2 * * *"） */
  cron_expr: string
  /** 任务配置（JSON 字符串，结构随 task_type 不同） */
  config: string
  /** 是否启用（停用后不再参与调度，但仍可手动运行） */
  enabled: boolean
  /** 最近一次运行时间（ISO 8601 字符串；空串表示从未运行） */
  last_run_at: string
  /** 最近一次运行状态：success / failed / unconfigured；空串表示从未运行 */
  last_status: string
  /** 最近一次运行的结果消息或"缺什么配置"的提示 */
  last_message: string
  /** 创建时间 */
  created_at: string
}

/** 应用内报告文件元信息 */
export interface ReportMeta {
  /** 文件名（不含路径，读取时回传作定位键） */
  name: string
  /** 文件大小（字节） */
  size: number
  /** 最后修改时间（展示用） */
  modified: string
}

/**
 * 手动生成报告的结果（对齐后端 `ReportGenResult`）
 *
 * 后端只在报告写入应用数据目录时回传文件名；Obsidian 通道的报告不在
 * 应用目录内，`file_name` 为 null，前端不应据此去读取。
 */
export interface ReportGenResult {
  /** 应用内可见的报告文件名（Obsidian 通道为 null） */
  file_name: string | null
  /** 结果描述（含落盘位置与事件统计） */
  message: string
}

/**
 * AI 流式增量事件载荷（事件名 `ai-stream`）
 *
 * 后端在生成摘要 / 翻译的过程中逐批推出，字段名与 Rust 侧 `AiStreamPayload`
 * 的 `rename_all = "camelCase"` 序列化一一对应（不是 snake_case）。
 */
export interface AiStreamPayload {
  /** 前端生成并回传的请求标识，用于丢弃过期请求的迟到事件 */
  requestId: string
  /**
   * 产物类型：界面据此决定把增量渲染到哪块区域
   *
   * - `summary`：AI 摘要卡片
   * - `translate`：正文译文
   * - `chat`：文章追问的回答气泡
   */
  kind: 'summary' | 'translate' | 'chat'
  /** 文章 ID */
  articleId: number
  /** 本批新增的文本（后端已按阈值合并，不是逐 token） */
  delta: string
}

/** 对话角色：用户提问 / 模型回答 */
export type ChatRole = 'user' | 'assistant'

/**
 * 一条文章追问问答
 *
 * 会话只存在于内存（刻意不落库），因此这里既是 UI 的渲染结构，也是回传给后端的
 * 历史消息形态——`role` / `content` 与后端命令的 `ChatTurn` 入参一一对应。
 */
export interface ChatMessage {
  /** 角色 */
  role: ChatRole
  /** 消息正文 */
  content: string
  /**
   * 是否为一次失败的提问
   *
   * 失败时仍把消息留在列表里（而不是静默丢弃），用户才能看见自己问了什么、
   * 并据此重试；渲染时以错误样式区分。
   */
  error?: boolean
  /**
   * 是否是一次被用户中途停止的回答
   *
   * 停止时保留已经生成的部分（往往已经包含有效信息），但必须标注出来——
   * 否则用户会把一段被截断的话当成完整回答。
   */
  stopped?: boolean
}
