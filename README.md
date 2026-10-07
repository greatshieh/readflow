# ReadFlow

> 本地优先的 AI RSS 阅读器 —— 无账户、无云端，数据与密钥全部留在你自己的机器上。

基于 **Tauri 2 + Vue 3 + Rust** 的桌面 RSS 阅读器。订阅、正文、高亮、AI 摘要与翻译、研究事件抽取，
全部在本机完成：网络请求、RSS 解析、AI 调用与 SQLite 读写都在 Rust 侧，前端只负责渲染与交互。

---

## 功能

**订阅与分组**
- 添加 / 编辑 / 删除订阅源，自动抓取站点图标并在图标失效时回退为首字母头像
- OPML 导入与导出
- 自动分组：按 RSSHub 路由前缀 → 配置的实例 → 主机名归组，绝不粗暴按域名切分
- 新增订阅时按候选实例列表顺序探测 RSSHub 连通性，失败静默回退首选实例，不打断操作

**阅读**
- 三栏布局（订阅 / 文章列表 / 正文），无边框自定义标题栏
- 按日期分组、已读与未读、收藏、全文搜索、分页加载
- 正文高亮：选中即变色标记，支持多色与逐条删除

**AI 增强**
- 单篇摘要、全文翻译，模型与 Base URL 完全自定义（DeepSeek / GLM / Qwen / Ollama 等任何
  OpenAI 兼容接口）
- 每日阅读简报、智能摘要分组

**研究流水线**
- 实体管理 → 单篇或批量事件抽取 → 时间线 → 报告阅读
- 报告支持日报 / 周报，可输出到应用内、Obsidian 或两者；实体共现关系图支持
  弹窗与主屏全宽两种视图
- 自动化任务（cron）驱动批量抽取与定时报告，全流程无人值守
- 详细用法见 [研究工作台使用指南](docs/research-guide.md)

**自动化与外观**
- 定时刷新与自定义自动化任务
- 明暗主题、强调色、界面字体 / 阅读字体 / 字号 / 缩放，全部可调
- 系统托盘（左键显示隐藏，右键菜单）、全局快捷键

**数据**
- 收录统计、数据库备份 / 恢复 / 清理
- 单向导出到 Obsidian

---

## 技术栈

| 层 | 选型 |
|---|---|
| 桌面框架 | Tauri 2（无边框窗口 + 系统托盘） |
| 前端 | Vue 3 `<script setup>` + TypeScript + Vite + Pinia |
| 后端 | Rust（tokio + sqlx + reqwest + chrono） |
| 数据库 | SQLite（本地单文件，无服务端） |

---

## 快速开始

### 环境要求

- Node.js ≥ 18，Rust ≥ 1.77
- **Linux** 还需安装 Tauri 的系统依赖：

  ```bash
  sudo apt update
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
      libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```

- **macOS**：`xcode-select --install`
- **Windows**：MSVC 工具链 + WebView2 Runtime

### 开发与运行

```bash
npm install
npm run tauri:dev      # 开发模式（自动在 1420 端口起 Vite）
npm run tauri:build    # 打包安装包（.deb / .AppImage / .dmg / .msi 等）
```

### 校验

```bash
npm run typecheck      # .vue + .ts 类型检查（vue-tsc）
npm run build:frontend # 前端打包
cd src-tauri && cargo test --lib && cargo check --lib
```

> `npm run build` = 上面「类型检查 + 打包」两步串联，供人工 / CI 使用；
> Tauri 的 `beforeBuildCommand` 只指向 `build:frontend`，类型检查不阻塞出包。

---

## 项目结构

```
src/                      前端
  components/             UI 组件（BaseModal / AppSwitch 等基础设施见 styles.css 与各自文件头）
  stores/                 Pinia：feeds / articles / tags / settings / research / ui …
  utils/                  appearance（外观应用）、dialogStack（弹窗计数）…
  styles.css              设计令牌单一来源：色彩 / 阴影 / 间距 / 字号
src-tauri/src/
  lib.rs                  IPC 边界：77 个命令的唯一注册与入口
  db.rs                   数据访问层与 Schema
  rss.rs                  RSS 抓取与正文清洗
  ai.rs                   AI 调用（摘要 / 翻译 / 日报简报 / 文章追问）
  scheduler.rs            定时刷新与自动化任务
  research/               实体 / 事件 / 报告（mod / extract / normalize / report）
  grouping.rs             订阅源自动分组
  rsshub.rs               RSSHub 实例探测与分组键推导
  opml.rs                 OPML 导入导出
  obsidian.rs             Obsidian 导出
  fonts.rs                系统字体枚举（零依赖解析 SFNT）
  notify.rs               系统通知（三重门：句柄 / 开关 / 窗口离开）
  net.rs                  共享 HTTP 客户端
  tray.rs                 系统托盘
  icons/                  应用图标（regen-icons.sh 可重出全套）
```

产品定位、阶段划分与模块拆解见 [`DEVELOPMENT_PLAN.md`](./DEVELOPMENT_PLAN.md)。

---

## 数据与隐私

- **无账户体系**，不连接任何自建服务端。
- 数据库位置：
  - Linux：`~/.local/share/cn.readflow.app/readflow.db`
  - macOS：`~/Library/Application Support/cn.readflow.app/readflow.db`
  - Windows：`%APPDATA%\cn.readflow.app\readflow.db`
- AI API Key 只写入本地 SQLite，设置页出于安全考虑**不回填明文**，仅以「已配置 / 未配置」提示状态。
- 正文一律使用 RSS 自带的 `content` / `summary`，**不做网页全文抓取**；入库 HTML 经 `ammonia` 清洗。

---

## 开发约定

- **前后端边界**：所有网络、RSS 解析、AI 与 SQLite 操作都在 Rust 侧；前端只做渲染、格式化与
  已加载数据的排序过滤，通过 `invoke('cmd', { camelCase })` 调用。
- **Schema 一次成型**：表结构直接写在 `db.rs::create_tables` 中，不使用 `ALTER TABLE` 补丁式迁移。
- **注释**：文件头写职责，函数头写「做什么 + 为什么 + 参数 + 返回 + 错误」；注释中文，标识符英文。
- **样式**：颜色、阴影、间距、字号一律引用 `styles.css` 的设计令牌，组件内禁止字面量色值。

---

## 许可证

本项目采用 MIT 许可证。
