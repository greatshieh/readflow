/**
 * 导出对象的类型定义
 *
 * # 为什么单独一个文件
 * `ReportExportMenu.vue` 用 `<script setup>`，而 `<script setup>` 里**不允许**
 * ES 模块导出（编译期直接报错），所以类型不能从组件文件里 import。
 * 类型放这里，组件与调用方共用同一份契约。
 *
 * # 两类报告共用同一形状
 * 周期报告走库（`id`），研究报告走文件（`name`）。导出管线只认"能从哪取到正文"，
 * 不认报告是新是旧——这是"两类报告共用一条导出管线"的前提。
 */

/** 导出对象 */
export interface ExportTarget {
  /** 报告来源：周期报告走库，研究报告走文件 */
  kind: 'period' | 'research'
  /** 周期报告 ID（`kind=period` 必填） */
  id?: number
  /** 研究报告文件名（`kind=research` 必填） */
  name?: string
  /** 菜单首行展示名（写清"导出的是哪一份"，防止在列表里导错对象） */
  label: string
}
