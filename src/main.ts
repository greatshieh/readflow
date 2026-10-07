/**
 * 应用入口（前端启动引导）
 *
 * # 职责
 * 创建 Vue 应用实例、注册全局插件（Pinia 状态管理）、挂载根组件 `App.vue`，
 * 并把应用挂到 `index.html` 中的 `#app` 容器上。这是整个前端渲染链路的起点，
 * 由 `vite` 打包后经 Tauri webview（或浏览器预览）加载执行。
 *
 * # 设计意图
 * - **入口保持极简**：任何初始化副作用都应下沉到组件或 store，
 *   避免入口文件变成难以测试的"上帝脚本"；数据加载由各组件在挂载时自行触发。
 * - **Pinia 全局注册一次**：三个 store（settings / articles / feeds）都通过
 *   `app.use(createPinia())` 注入的同一个 pinia 实例取用，保证跨组件状态共享。
 *
 * # 运行环境说明
 * 本应用为 Tauri 桌面应用，所有数据均来自 Rust 后端（`invoke` 调用），**不存在 mock 数据回退**。
 * - 真实 Tauri 桌面应用（`tauri dev` / 打包产物）：本文件在 Tauri webview 中执行，
 *   此时 `window.__TAURI_INTERNALS__` 已由预加载脚本注入，各 store 的 `invoke` 可用。
 * - 浏览器预览（`npm run dev`）：同一份代码在普通浏览器中执行时 `invoke` 不可用，
 *   调用 store 会直接失败——这是预期行为，本应用不提供 mock 数据，**界面开发与调试应在 `tauri dev` 环境进行**。
 *
 * # 注意事项
 * - 样式入口 `./styles.css` 在此处统一导入，组件内无需重复引入全局样式。
 * - 当前**没有**在前端注册任何 Tauri 插件（如 `@tauri-apps/plugin-*` 的 JS 绑定）。
 *   后端 `Cargo.toml` 中声明的插件（如 tauri-plugin-shell）在 Rust 侧注册即可供
 *   `invoke` 使用；若后续引入需要 JS 侧初始化的插件，应在此处挂载并记录其权限影响。
 */

import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import './styles.css'

// 创建应用实例，并在挂载前完成插件注册（顺序很重要：
// 必须先 use(pinia)，否则组件 setup 中调用 useXxxStore() 时拿不到 active pinia）
const app = createApp(App)
app.use(createPinia())
app.mount('#app')
