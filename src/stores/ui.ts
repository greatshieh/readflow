/**
 * UI 状态 store
 *
 * # 职责
 * 存放与"界面呈现"相关的临时状态，这类状态只影响当前会话的视图，
 * 不需要持久化，也不属于业务数据（订阅源 / 文章 / 设置），因此独立成 store，
 * 避免污染 feeds / articles / settings 三个业务 store。
 *
 * 目前承载：
 * - `focusMode`：专注阅读模式开关。开启时隐藏左侧订阅列表与文章列表，
 *   把正文区域加宽，减少视觉干扰、便于长文阅读。
 * - `mainView`：主屏（订阅栏右侧区域）当前展示哪一种内容——文章阅读
 *   还是实体关系图。关系图在弹窗里画布太小，故提供全宽的主屏展示位。
 *
 * # 设计意图
 * 专注模式需要同时作用于两个不同层级的组件（App.vue 的 FeedPanel 与
 * ArticleColumn.vue 的文章列表），用 Pinia 全局单例比"逐层透传 props / 派发事件"
 * 更简洁，也避免组件间耦合。任何组件 `useUiStore()` 即可读写同一份状态。
 */

import { defineStore } from 'pinia'
import { ref } from 'vue'

export const useUiStore = defineStore('ui', () => {
  /**
   * 专注阅读模式开关
   *
   * true = 进入专注模式：隐藏订阅列表与文章列表，正文区域占据全部宽度。
   * 初始为 false（正常三栏布局）。
   */
  const focusMode = ref(false)

  /** 主屏内容类型：articles = 文章列表 + 正文（默认三栏）；graph = 实体关系图（全宽） */
  const mainView = ref<'articles' | 'graph'>('articles')

  /**
   * 切换主屏内容类型
   *
   * 由关系图的两个入口（研究工作台图页签的「放大到主屏」、侧栏「更多」菜单）
   * 与主屏图视图的「返回文章」按钮调用。
   *
   * @param view - 目标视图：'articles' 回到阅读，'graph' 进入关系图
   * @returns 无返回值；副作用为覆盖 mainView，App.vue 据此切换渲染的栏组件
   */
  function setMainView(view: 'articles' | 'graph') {
    mainView.value = view
  }

  /**
   * 切换专注模式
   *
   * 用于正文区域的专注按钮点击：在"正常 / 专注"两种布局间来回切换。
   *
   * @returns 无返回值；副作用为翻转 focusMode
   */
  function toggleFocusMode() {
    focusMode.value = !focusMode.value
  }

  /**
   * 强制设置专注模式
   *
   * 用于全局快捷键（如 Esc 退出专注模式）等需要明确置位的场景。
   *
   * @param value - 目标状态：true 进入专注，false 退出专注
   * @returns 无返回值；副作用为覆盖 focusMode
   */
  function setFocusMode(value: boolean) {
    focusMode.value = value
  }

  /* ─── 三栏列宽（持久化到 localStorage，对齐 Folo 的 feedColWidth / entryColWidth）───
     订阅栏与文章列表栏的宽度可由分隔条拖拽调整，会话间保持；正文栏始终 flex:1 自适应。
     这类纯布局偏好不属于业务数据，故仅存 localStorage，不入 SQLite settings 表。 */
  const FEED_W_KEY = 'readflow:feedColWidth'
  const ARTICLE_W_KEY = 'readflow:articleColWidth'
  /** 订阅栏（左列）宽度，默认 260px */
  const feedColWidth = ref(Number(localStorage.getItem(FEED_W_KEY)) || 260)
  /** 文章列表栏（中列）宽度，默认 360px */
  const articleColWidth = ref(Number(localStorage.getItem(ARTICLE_W_KEY)) || 360)

  /** 设置并持久化订阅栏宽度，拖拽分隔条时实时调用 */
  function setFeedColWidth(w: number) {
    feedColWidth.value = w
    localStorage.setItem(FEED_W_KEY, String(w))
  }
  /** 设置并持久化文章列表栏宽度，拖拽分隔条时实时调用 */
  function setArticleColWidth(w: number) {
    articleColWidth.value = w
    localStorage.setItem(ARTICLE_W_KEY, String(w))
  }

  return { focusMode, toggleFocusMode, setFocusMode, mainView, setMainView, feedColWidth, articleColWidth, setFeedColWidth, setArticleColWidth }
})
