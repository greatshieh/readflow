/**
 * 全局环境类型声明
 *
 * # 职责
 * 为没有自带类型定义、但又需要在前端代码中访问的全局对象补充类型声明，
 * 让 TypeScript 编译期认识它们。本文件只含 `declare global`，是纯类型声明，
 * 不产出任何运行时代码。
 *
 * # 设计意图
 * - 声明 `process.env` 是为了让读取环境变量（如构建期注入的开关）时能获得
 *   字符串类型提示；Vite 生态中更常见的是 `import.meta.env`，
 *   本声明属于对 Node 风格写法的兼容兜底。
 * - 与 `vite-env.d.ts` 分工明确：后者通过 `/// <reference types="vite/client" />`
 *   批量引入 Vite 官方客户端类型（含 `import.meta.env`、静态资源模块声明等），
 *   本文件只补充项目自定义的全局变量。
 *
 * # 注意事项
 * - 这仅是**类型层面的声明**，不代表 Tauri webview 运行时真的存在 Node 的 `process`；
 *   运行时若未注入该对象，解引用仍会得到 undefined，需配合构建期的 define 使用。
 */

// 声明 Node 风格的环境变量对象：键与值统一为字符串，
// 需要布尔/数字时由调用方自行转换。
declare global { const process: { env: Record<string, string> } }
