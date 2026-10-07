# ReadFlow 开发计划

## 一、项目概述

### 1.1 产品定位
本地优先的 AI RSS 阅读器，专注于：
- 隐私保护：无账户体系，数据完全本地存储
- AI 增强：自定义 AI 模型，支持摘要和翻译
- 生态集成：首个目标 Obsidian，单向导出文章
- 桌面原生：Tauri 2.x，跨平台（macOS/Windows/Linux）

### 1.2 技术栈
- **前端框架**：Vue 3 + TypeScript + Vite
- **后端运行时**：Rust (tokio + sqlx)
- **数据库**：SQLite（本地文件）
- **桌面框架**：Tauri 2.x
- **UI 组件**：Naive UI 风格（自实现）

### 1.3 核心差异点
1. 完全本地化，无云端依赖
2. AI 模型完全自定义（DeepSeek/GLM/Qwen/Ollama）
3. Obsidian 单向导出（首个集成目标）
4. 参考 Folio 双栏布局 + Naive UI 风格

---

## 二、功能模块分解

### Phase 1: 项目骨架搭建（P0 - 必做）

#### 1.1 Tauri 项目初始化
- [ ] 创建项目结构：`npm create tauri-app@latest readflow -- --template vue-ts`
- [ ] 配置 `tauri.conf.json`：无边框窗口、CSP 设置
- [ ] 配置 `Cargo.toml`：添加依赖（sqlx, tokio, reqwest, chrono）
- [ ] 验证开发模式运行：`npm run tauri dev`

#### 1.2 数据库层设计
- [ ] 创建数据库连接池管理模块
- [ ] 设计 SQLite Schema（feeds, articles, settings 表）
- [ ] 实现基础 CRUD 封装
- [ ] 实现连接池管理（应用级单例）

**数据库 Schema 设计**：

```sql
-- 订阅源表
CREATE TABLE IF NOT EXISTS feeds (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    url TEXT NOT NULL UNIQUE,
    icon TEXT DEFAULT '',
    category TEXT DEFAULT '未分类',
    fetch_interval INTEGER DEFAULT 3600,
    last_fetch_at DATETIME,
    next_fetch_at DATETIME,
    unread_count INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- 文章表
CREATE TABLE IF NOT EXISTS articles (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    feed_id INTEGER NOT NULL,
    guid TEXT NOT NULL,
    link TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT DEFAULT '',
    content TEXT DEFAULT '',
    author TEXT DEFAULT '',
    published_at DATETIME,
    is_read INTEGER DEFAULT 0,
    is_bookmarked INTEGER DEFAULT 0,
    has_ai_summary INTEGER DEFAULT 0,
    has_ai_translation INTEGER DEFAULT 0,
    ai_summary TEXT,
    ai_translation TEXT,
    ai_model TEXT DEFAULT '',
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (feed_id) REFERENCES feeds(id) ON DELETE CASCADE,
    UNIQUE(feed_id, guid)
);

-- 索引优化
CREATE INDEX IF NOT EXISTS idx_articles_feed_id ON articles(feed_id);
CREATE INDEX IF NOT EXISTS idx_articles_is_read ON articles(is_read);
CREATE INDEX IF NOT EXISTS idx_articles_published_at ON articles(published_at DESC);

-- 设置表
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

#### 1.3 前端项目结构
- [ ] 配置 TypeScript：`tsconfig.json`
- [ ] 配置 Vite：`vite.config.ts`
- [ ] 创建目录结构：
  ```
  src/
  ├── components/        # UI 组件
  │   ├── TitleBar.vue
  │   ├── FeedPanel.vue
  │   ├── ArticleList.vue
  │   ├── Content.vue
  │   └── SettingsModal.vue
  ├── stores/            # Pinia 状态管理
  │   ├── useFeeds.ts
  │   ├── useArticles.ts
  │   └── useSettings.ts
  ├── types/             # TypeScript 类型
  │   └── index.ts
  ├── utils/             # 工具函数
  │   └── format.ts
  ├── App.vue
  └── main.ts
  ```

#### 1.4 全局样式系统
- [ ] CSS 变量定义（颜色、间距、圆角）
- [ ] 响应式布局（>1024px / 768-1024px / <768px）
- [ ] Naive UI 风格组件基类
- [ ] 滚动条样式统一

---

### Phase 2: 订阅管理（P0 - 必做）

#### 2.1 后端 API
- [ ] `feeds/list` - 获取订阅列表
- [ ] `feeds/add` - 添加订阅源
- [ ] `feeds/delete` - 删除订阅源
- [ ] `feeds/update` - 更新订阅信息
- [ ] `feeds/import-opml` - OPML 导入
- [ ] `feeds/export-opml` - OPML 导出

#### 2.2 RSS 解析器
- [ ] 实现 RSS 2.0 解析器
- [ ] 实现 Atom 格式解析器
- [ ] 实现 JSON Feed 解析器
- [ ] 相对链接转绝对链接处理
- [ ] 标题/摘要/内容提取

#### 2.3 前端组件
- [ ] 订阅列表展示（带未读计数）
- [ ] 添加订阅对话框（URL 输入 + 自动识别）
- [ ] 订阅搜索/过滤
- [ ] OPML 导入导出按钮
- [ ] 分类管理（标签/文件夹）

#### 2.4 交互流程
```
用户添加订阅
  ↓
前端提交 URL
  ↓
后端 fetch RSS → 解析
  ↓
获取标题、icon、分类
  ↓
写入数据库
  ↓
返回 Feed 对象
  ↓
前端更新列表
```

---

### Phase 3: 文章获取与刷新（P1 - 核心）

#### 3.1 后台刷新机制
- [ ] 实现定时任务调度（tokio-cron-scheduler）
- [ ] 支持自定义刷新间隔（每小时/每天等）
- [ ] 增量更新策略（只拉取新文章）
- [ ] 错误重试机制（指数退避）
- [ ] 刷新进度通知（Tauri 事件）

#### 3.2 文章内容提取
- [ ] 集成 readability crate
- [ ] 去除导航、广告、侧边栏
- [ ] 保留标题、正文、图片
- [ ] 处理 iframe/嵌入内容

#### 3.3 文章缓存策略
- [ ] 文章本地存储（SQLite）
- [ ] 离线可读（已缓存文章）
- [ ] 定期清理旧文章（可配置）
- [ ] 磁盘空间监控

#### 3.4 前端展示
- [ ] 文章列表分页/无限滚动
- [ ] 未读标记（红点）
- [ ] 阅读历史追踪
- [ ] 排序选项（最新/重要/名称）
- [ ] 搜索过滤（标题/作者/内容）

---

### Phase 4: 文章阅读（P1 - 核心）

#### 4.1 内容渲染
- [ ] HTML 内容安全渲染
- [ ] Markdown 支持（可选）
- [ ] 图片懒加载
- [ ] 代码高亮（可选）
- [ ] 字体大小调节
- [ ] 主题切换（明/暗）

#### 4.2 阅读体验
- [ ] 阅读进度记录
- [ ] 目录导航（标题锚点）
- [ ] 字数统计/预计阅读时间
- [ ] 打印优化样式

#### 4.3 交互功能
- [ ] 书签管理
- [ ] 分享功能（复制链接）
- [ ] 原文链接打开
- [ ] 快捷键支持

---

### Phase 5: AI 功能（P2 - 差异化）

#### 5.1 AI 客户端架构
- [ ] OpenAI 兼容接口封装
- [ ] 多提供商支持（DeepSeek、GLM-4、Qwen）
- [ ] 本地模型支持（Ollama）
- [ ] API 密钥加密存储
- [ ] 请求重试/超时处理

**AI 提供商配置示例**：
```typescript
interface AIProvider {
  id: string;
  name: string;
  displayName: string;
  api_url: string;
  api_key: string;
  models: AIMode[];
}

interface AIMode {
  id: string;
  name: string;
  max_tokens: number;
  supports_streaming: boolean;
}
```

#### 5.2 AI 摘要功能
- [ ] 全文摘要生成
- [ ] 多长度选项（简短/标准/详细）
- [ ] 流式输出（SSE）
- [ ] 摘要缓存（避免重复请求）
- [ ] 切换摘要/原文显示

#### 5.3 AI 翻译功能
- [ ] 整篇文章翻译（非逐段）
- [ ] 手动切换翻译/原文
- [ ] 支持多种目标语言
- [ ] 翻译缓存
- [ ] 翻译质量提示

#### 5.4 前端集成
- [ ] AI 面板 UI（摘要/翻译标签）
- [ ] 生成状态指示器
- [ ] 错误处理提示
- [ ] 设置页面（API 配置）

---

### Phase 6: Obsidian 集成（P3 - 特色）

#### 6.1 Obsidian URI 支持
- [ ] 检查 Obsidian 是否安装
- [ ] 检测 Vault 列表
- [ ] 自动生成 URI：`obsidian://open?vault=xxx&file=yyy`

#### 6.2 文章导出
- [ ] 导出为 Markdown 文件
- [ ] 元数据注入（日期、来源、标签）
- [ ] 图片本地化（可选）
- [ ] 异步导出（后台任务）

#### 6.3 导出设置
- [ ] 默认 Vault 选择
- [ ] 导出目录配置
- [ ] 文件名模板
- [ ] 标签映射规则

---

### Phase 7: 设置与数据管理（P2 - 必需）

#### 7.1 设置项
- [ ] AI 模型配置（提供商、API Key、模型选择）
- [ ] 刷新频率设置
- [ ] 主题/样式偏好
- [ ] 字体大小
- [ ] 文章保留策略（自动清理）

#### 7.2 数据管理
- [ ] 数据库备份（导出 SQLite 文件）
- [ ] 数据库恢复（导入备份）
- [ ] OPML 导入/导出
- [ ] 数据统计（文章总数、已读比例等）

---

## 三、开发里程碑

### M1: 骨架可用（Week 1-2）
- [ ] Tauri + Vue 项目跑通
- [ ] 数据库连接正常
- [ ] 基本 UI 框架完成
- [ ] 可以添加/删除订阅源

### M2: 订阅管理（Week 3-4）
- [ ] RSS 解析正常工作
- [ ] 订阅列表展示
- [ ] OPML 导入导出
- [ ] 文章列表展示

### M3: 文章阅读（Week 5-6）
- [ ] 文章内容渲染
- [ ] 阅读状态标记
- [ ] 搜索/过滤功能
- [ ] 书签管理

### M4: AI 功能（Week 7-8）
- [ ] AI 摘要生成
- [ ] AI 翻译功能
- [ ] 设置页面
- [ ] 流式输出优化

### M5: 增强功能（Week 9-10）
- [ ] Obsidian 导出
- [ ] 数据备份恢复
- [ ] 性能优化
- [ ] Bug 修复

---

## 四、关键实现细节

### 4.1 数据库连接池管理

```rust
// src/db/mod.rs
use sqlx::{SqlitePool, Pool};
use std::sync::Arc;

pub struct Db {
    pool: Pool<sqlx::Sqlite>,
}

impl Db {
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(database_url).await?;
        Ok(Db { pool })
    }

    pub fn pool(&self) -> &Pool<sqlx::Sqlite> {
        &self.pool
    }
}
```

### 4.2 RSS 抓取与解析

```rust
// src/fetcher/rss.rs
use reqwest::Client;
use rss::Channel;

pub async fn fetch_feed(url: &str) -> Result<FeedData, FetchError> {
    let client = Client::new();
    let resp = client.get(url).send().await?;
    let body = resp.text().await?;

    let channel = Channel::read_from(body.as_bytes())?;

    Ok(FeedData {
        title: channel.title().to_string(),
        link: channel.link().to_string(),
        items: channel.items().iter().map(|item| {
            Item {
                title: item.title().to_string(),
                link: item.link().unwrap_or_default(),
                guid: item.guid().map(|g| g.value().to_string()),
                // ...
            }
        }).collect(),
    })
}
```

### 4.3 AI 请求封装

```rust
// src/ai/client.rs
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct ChatRequest {
    model: String,
    messages: Vec<Message>,
}

#[derive(Deserialize)]
pub struct ChatResponse {
    choices: Vec<Choice>,
}

impl AIProvider {
    pub async fn generate_summary(&self, content: &str) -> Result<String, AIError> {
        let prompt = format!(
            "请为以下文章生成摘要（100字以内）：\n\n{}",
            content
        );

        let response = self.client.post(&self.api_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&ChatRequest {
                model: self.model.clone(),
                messages: vec![Message {
                    role: "user".to_string(),
                    content: prompt,
                }],
            })
            .send()
            .await?;

        Ok(response.text().await?)
    }
}
```

### 4.4 前端状态管理（Pinia）

```typescript
// src/stores/useFeeds.ts
import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { Feed } from '@/types'

export const useFeedsStore = defineStore('feeds', () => {
  const feeds = ref<Feed[]>([])
  const selectedFeed = ref<Feed | null>(null)

  async function loadFeeds() {
    feeds.value = await invoke('feeds_list')
  }

  async function addFeed(url: string) {
    const id = await invoke('feeds_add', { url })
    await loadFeeds()
    return id
  }

  return { feeds, selectedFeed, loadFeeds, addFeed }
})
```

---

## 五、风险与应对

| 风险 | 影响 | 应对策略 |
|------|------|----------|
| SQLite 并发写限制 | 中等 | 单连接 + 队列化处理 |
| RSS 源格式不统一 | 高 | 多格式解析器 + 降级策略 |
| AI API 不稳定 | 中 | 重试机制 + 本地缓存 |
| Tauri 跨平台差异 | 低 | 条件编译 + 平台检测 |
| 大文章内存占用 | 中 | 分页加载 + 流式处理 |

---

## 六、待确认事项

- [ ] RSSHub 是否需要集成？
- [ ] AI 功能是否需要同时支持摘要和翻译？
- [ ] 订阅源 discovery 方式（手动添加 vs 搜索发现）？
- [ ] 是否需要用户登录系统？
- [ ] 数据是否需要云端备份？

---

## 七、下一步行动

### 7.1 环境准备

**安装 Rust：**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
rustc --version
```

**安装 Node.js：**
```bash
# macOS
brew install node

# Linux (Ubuntu)
curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
sudo apt-get install -y nodejs
```

**验证环境：**
```bash
rustc --version  # rustc 1.75+
node --version   # v20+
npm --version    # 10+
```

### 7.2 项目初始化

```bash
cd /home/ivan/CodeNexus/readflow
npm create tauri-app@latest readflow -- --template vue-ts --manager npm
cd readflow
npm install
```

### 7.3 开始 Phase 1

按照计划逐步实现各个功能模块。

---

## 八、代码规范

### 8.1 Rust 命名规范
- 函数：`snake_case`
- 结构体：`PascalCase`
- 常量：`UPPER_SNAKE_CASE`
- 类型别名：`PascalCase`

### 8.2 TypeScript 命名规范
- 接口：`PascalCase`
- 类型别名：`PascalCase`
- 函数：`camelCase`
- 常量：`UPPER_SNAKE_CASE`

### 8.3 Git 提交规范
```
feat: 新增功能
fix: 修复 bug
docs: 文档更新
style: 代码格式调整
refactor: 重构
test: 测试相关
chore: 构建/工具相关
```

---

*文档版本：v1.0*
*最后更新：2026-09-23*
