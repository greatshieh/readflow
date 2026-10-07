/**
 * Vite 客户端类型与 Vue 单文件组件（SFC）类型声明
 *
 * # 职责
 * 1. 通过三斜杠指令引入 Vite 官方的客户端类型，让 `import.meta.env`、
 *    `import.meta.hot` 以及各类静态资源导入（如 .svg / .png / .css）
 *    在 TypeScript 下可识别。
 * 2. 为 `*.vue` 模块补充类型声明，使 `import App from './App.vue'` 能被正确推断。
 *
 * # 设计意图
 * - 这是 Vite + Vue + TS 项目的标准环境声明文件：本身不产出运行时代码，
 *   仅参与编译期类型检查（如 `vue-tsc --noEmit`）。
 * - `*.vue` 统一声明为 `DefineComponent<{}, {}, any>`：牺牲了 props 的精确推断，
 *   换来的是不必为每个 SFC 生成 `.vue.d.ts`；组件级别的精确类型由
 *   `<script setup lang="ts">` 内部自行保证。
 *
 * # 注意事项
 * - 本文件必须保持"无 import/export 之外的副作用"且被 tsconfig 的 include 覆盖，
 *   否则类型声明会失效。
 * - 若在组件中使用 Tauri 插件或自定义 Vite 插件注入的 `import.meta` 字段，
 *   应在此处或 `env.d.ts` 中补充对应声明，而不是在组件里用 `as any` 绕过。
 */

/// <reference types="vite/client" />

// 为所有 .vue 单文件组件提供兜底类型：默认导出一个 Vue 组件构造器。
// 这里用宽泛的 DefineComponent<{}, {}, any> 以兼容任意 props/emits 定义。
declare module '*.vue' {
  import type { DefineComponent } from 'vue'
  const component: DefineComponent<{}, {}, any>
  export default component
}
