# 弹窗改版 · 落地影响面与风险清单（开发对照用）

> **这份文档给谁看**：拿到设计稿后要动代码的开发。
> **它不重复设计判断**（字号多大、动效多长、留白多少由设计稿定）。它只回答三个问题：
> **① 一处改动会连带动到哪些文件（精确到次数）；② 设计稿里这类方案在代码里实施会撞到什么；③ 改完怎么自查。**
>
> **数据快照**：2026-10-30，工作树 `eba74d3`（`docs(theme): 清理紫罗兰单色化后残留的琥珀描述…`）。
> 全部数字是**在该工作树上重新跑命令复算的**，节末都附了可重跑的命令——数字会随代码漂移，**以你改之前重跑的结果为准**。
> 行号只用于定位，不是契约。

---

## 0. 四条结论（先看这个，再决定动哪里）

1. **「把弹窗字号调大」没有单一开关**：弹窗相关字号声明 **151 处** = 23 个 `.vue` 文件内 **128 处** + `styles.css` 里只被弹窗消费的规则 **23 处**。
   在 `.base-modal` 上覆盖 `--fs-*` 能一次覆盖其中绝大部分，**但会让 `AppSelect` 的触发器与它的下拉面板字号分叉**（下拉 `Teleport` 到 body，不继承覆盖值）——见 R1。
2. **唯一"改了一定不连带主界面"的单点是外壳本身**：`BaseModal.vue` 有 **0 条字号声明 + 6 条外观声明 + 1 个 `@keyframes`**。改圆角 / 玻璃 / 进出场动效只动这一个文件。
   **字号与内边距不在此列**，必须逐组件改或走作用域覆盖。
3. **弹窗不跟随「界面字体大小」设置**——这是既有事实，不是这次要改出来的：应用根 `.app` 有 `zoom: var(--ui-zoom)`（0.9 / 1 / 1.1 / 1.2），而弹窗 `Teleport to="body"`，**落在 zoom 容器之外**（`styles.css:448` + `BaseModal.vue:10`）。
   用户把字号调到"大"，主界面放大 10%、弹窗一点不变。若本次要解决"字体偏小 / 呆板"，这一条必须先在设计侧定：**要么给弹窗补上缩放（一处改动，全弹窗生效），要么明确"弹窗固定视觉尺度"**。二者观感完全不同，别在实施时临时选。
4. **动字号之前有一个硬前置**：面板是 `max-height: 86vh` + `overflow: hidden`（`BaseModal.vue:271` / `:285`），而 **6 个弹窗没有内部滚动容器**（`EntityAlertRuleModal` / `FeedAddModal` / `FeedManageModal` / `GlobalModal` / `InsightsModal` / `OnboardingModal`）⇒ 字一放大，**超出的部分直接消失且滚不到**，底部按钮跟着一起没。这是"改法对了也会坏"的一条，**必须先在 `1366×768` 上验**。详见 §5.1。

---

## 1. 影响面：精确数字

### 1.1 弹窗到底有几个：13 个文件属于"弹窗外壳体系"

**渲染 `<BaseModal>` 的组件 12 个** + `BaseModal` 自身 = **13 个文件**：

| # | 文件 | 宽度档 | 布局 | 说明 |
|---|---|---|---|---|
| 1 | `BaseModal.vue` | sm/md/lg/xl/2xl/3xl | default/side | 唯一外壳，遮罩 / 动效 / 焦点陷阱都在这里 |
| 2 | `SettingsModal.vue` | — | **side** | 左 tab 列 + 右内容列，高 `min(80vh,720px)` |
| 3 | `ResearchModal.vue` | — | **side** | 同上，三个 tab |
| 4 | `EntityDashboardModal.vue` | **3xl** | default | 最复杂：24 个全局 `.tech-*` 类 + 内嵌 `MarketPanel` |
| 5 | `DigestModal.vue` | lg | default | |
| 6 | `InsightsModal.vue` | xl | default | 含 SVG 图表（`.ins-axis` 11px 硬编码） |
| 7 | `RulesModal.vue` | sm | default | |
| 8 | `FeedAddModal.vue` | sm | default | |
| 9 | `FeedManageModal.vue` | sm | default | |
| 10 | `EntityAlertRuleModal.vue` | lg | default | 由 `ResearchEntitiesTab` 打开（弹窗套弹窗的场景） |
| 11 | `GlobalModal.vue` | sm | default | 全局确认框，`z-index: 9999` |
| 12 | `ShortcutsModal.vue` | md | default | |
| 13 | `OnboardingModal.vue` | md | default | 首启引导 |

```bash
# 复核
grep -rln "<BaseModal" src --include="*.vue"      # 12 行
```

**⚠️ `TagPicker.vue` 不是弹窗**。它是正文工具条下方的**浮层**（`.tag-picker`，不 Teleport、不是 `role="dialog"`、不锁滚动、Esc 不归它管，唯一宿主是 `ContentToolbar.vue`）。
设计稿若把"浮层"和"弹窗"当一类改，改的其实是阅读界面里的一个常驻浮层。**评审阶段就要分清这两类。**

**弹窗内的子组件（9 个弹窗专用文件 47 处 + 1 个共享组件 `AppSelect` 3 处 = 50 处）**：

| 子组件 | 被谁用 | 字号声明 |
|---|---|---|
| `SettingsGeneralTab` / `SettingsAiTab` / `SettingsDataTab` / `SettingsObsidianTab` / `SettingsAutomationTab` | 只被 `SettingsModal` | 5 + 1 + 2 + 0 + 5 = **13** |
| `ResearchTimelineTab` / `ResearchReportsTab` / `ResearchEntitiesTab` | 只被 `ResearchModal`（`ResearchEntitiesTab` 还被 `EntityAlertRuleModal` 用） | 13 + 4 + 8 = **25** |
| `MarketPanel` | 只被 `EntityDashboardModal` | **9** |
| `AppSelect` / `AppSwitch` | 弹窗内 8 个文件 + 主界面 `GraphView` / `AppTooltip` | 3 + 0（**共享，不能算作"弹窗专用"**） |

改弹窗字号时最容易漏的就是这张表：**12 个宿主共 78 处，另有 47 处藏在这些子组件里**（占弹窗侧全部 128 处的 37%）。只改宿主、不管 tab，等于改了一半。

### 1.2 `--fs-*` 全仓 240 处：弹窗相关 151 / 主界面 81 / 全局 8 → **不能直接改 token 值**

| 分类 | xs | sm | md | base | lg | xl | 2xl | 合计 |
|---|---|---|---|---|---|---|---|---|
| **弹窗相关** | 43 | 64 | 23 | 13 | 3 | 3 | 2 | **151** |
| └ 23 个 `.vue` 文件内（含 tab / MarketPanel / AppSelect） | 35 | 56 | 21 | 11 | 2 | 2 | 1 | 128 |
| └ `styles.css` 里只被弹窗消费的规则（`.tech-*` 14 + `.setting*`/`.form*` 等 9） | 8 | 8 | 2 | 2 | 1 | 1 | 1 | 23 |
| **主界面 `.vue`**（21 个文件） | 27 | 39 | 8 | 3 | 2 | 1 | 1 | **81** |
| **`styles.css` 全局规则**（`body` / `.btn` / `.btn-mini` / `.graph-*` / 订阅栏空态 / `.state-hint`） | 3 | 4 | 1 | 0 | 0 | 0 | 0 | **8** |
| 全仓 | 73 | 107 | 32 | 16 | 5 | 4 | 3 | **240** |

**为什么"直接改 `--fs-sm: 13px → 14px`"是个陷阱**：`--fs-sm` 有 107 处引用，其中最要命的是
`styles.css:611` 的 `.btn { font-size: var(--fs-sm) }` —— `.btn` 在 **16 个文件 / 42 处**出现（口径：**`class` / `:class` 属性里含独立 token `btn`**，逐个属性数一次；若把 `.btn-primary` / `.btn-sm` 这些修饰类的宿主元素也算进来，是 57 处 / 18 文件）。
改一个 token 值 = 全应用按钮同时变，**与"只改弹窗"完全相反**。

同理：`--fs-md` 是 `body` 的基准字号（`styles.css:433`），改它等于改全应用默认文字。

> 结论：**token 值不动**。要改弹窗字号只有两条路——① 在 `.base-modal` 作用域重定义 `--fs-*`（风险见 R1）；② 逐组件改声明（151 处里挑设计稿真正涉及的）。

```bash
# 复核（逐 token 计数）
for t in xs sm md base lg xl 2xl; do printf "%-9s %s\n" "--fs-$t" "$(grep -ro "var(--fs-$t)" src --include="*.vue" --include="*.css" | wc -l)"; done
# 合计 240
```

### 1.3 12 个弹窗宿主的字号分布（78 处；外壳 0 处）

| 文件 | xs | sm | md | base | lg | xl | 2xl | 合计 |
|---|---|---|---|---|---|---|---|---|
| DigestModal | 2 | 4 | 2 | 2 | 1 | 0 | 0 | 11 |
| EntityDashboardModal | 5 | 4 | 2 | 2 | 0 | 0 | 0 | 13 |
| EntityAlertRuleModal | 0 | 8 | 1 | 1 | 0 | 0 | 0 | 10 |
| RulesModal | 3 | 4 | 1 | 0 | 0 | 0 | 0 | 8 |
| FeedManageModal | 3 | 4 | 1 | 0 | 0 | 0 | 0 | 8 |
| ShortcutsModal | 3 | 1 | 1 | 0 | 1 | 0 | 0 | 6 |
| FeedAddModal | 3 | 1 | 1 | 0 | 0 | 0 | 0 | 5 |
| InsightsModal | 2 | 2 | 1 | 0 | 0 | 0 | 0 | 5 |
| GlobalModal | 1 | 2 | 1 | 0 | 0 | 0 | 0 | 4 |
| SettingsModal | 0 | 1 | 1 | 1 | 0 | 0 | 0 | 3 |
| ResearchModal | 0 | 1 | 1 | 1 | 0 | 0 | 0 | 3 |
| OnboardingModal | 0 | 0 | 0 | 1 | 0 | 1 | 0 | 2 |
| **BaseModal（外壳）** | 0 | 0 | 0 | 0 | 0 | 0 | 0 | **0** |
| **合计** | **22** | **32** | **13** | **8** | **2** | **1** | **0** | **78** |

两点值得注意：

- **`BaseModal` 自己一条字号声明都没有**。这不是遗漏，是设计：面板不自带字号，全部继承 `body`（14px）后由各弹窗声明。所以"统一弹窗字号"在结构上**必须**靠作用域覆盖（R1）或逐组件改。
- **`--fs-2xl`（22px）在 13 个弹窗宿主里被用了 0 次**：弹窗标题实际用 `--fs-lg` / `--fs-xl`，关闭钮用**硬编码 22px**（见 1.4）。
  设计稿若写"弹窗标题 22px"，可引入 `--fs-2xl`；**但别改它的值**——它同时被 `ContentColumn.vue:894`（正文 h1，主界面）、`MarketPanel.vue:292`（仪表盘 KPI）、`styles.css:1344`（`.tech-hero-title`）用着，全仓仅 3 处。

### 1.4 硬编码字号 37 处，其中**弹窗相关 13 处**

| 值 | 位置 | 元素 | 弹窗相关 |
|---|---|---|---|
| **22px** ×5 | `DigestModal:244` `.m-close`、`FeedAddModal:158` `.add-close`、`FeedManageModal:270` `.manage-close`、`RulesModal:434` `.manage-close`、`GlobalModal:228` `.modal-close-btn` | 关闭按钮 `×` | ✅ |
| `1.5rem` | `InsightsModal:255` `.ins-num` | 大数字指标（= 24px，弹窗内唯一一处 `rem`） | ✅ |
| `11px` | `InsightsModal:322` `.ins-axis` | SVG 轴标签 | ✅ |
| `15px` ×2 | `SettingsGeneralTab:704` `.tag-remove`、`ResearchTimelineTab:616` | 小图标按钮 | ✅ |
| `em` ×4 | `ResearchReportsTab:354 / 356 / 361 / 377`（`.md :deep(h1/h2/h3/…)`） | Markdown 正文等比缩放 | ✅ |
| `11px` | `EntityGraph:746` | 关系图图例 | ❌ |
| `18px` / `12px`×3 / `13px` / `14px`×3 / `15px` / `11px`×2 | `RecommendView:223/229/238/253/286/290/298/324/339/350/367`（**一个文件 11 处**） | 推荐页 | ❌ |
| `9px` | `ArticleListItem:288` | 角标 | ❌ |
| `13px` | `ArticleListToolbar:255` `.tb-tag-x` | 标签删除钮 | ❌ |
| `inherit` | `CommandPalette:927` | 命令面板 | ❌ |
| `calc(... * var(--reading-scale))` ×4 | `ContentColumn:766/882`、`ArticleSummaryCard:129` | 阅读区字号（**刻意用 `--reading-scale`**） | ❌ |
| `em` ×4 | `ContentColumn:912/917/918/920` | Markdown 正文 | ❌ |
| `15px` ×2 | `TagPicker:318 / 393` | 浮层（不是弹窗） | ❌ |

**要说清的三件事**：

1. **"9 处"是只数了 `px` 里最显眼的那批**。穷尽是 **37 处**：`22px`×5、`15px`×5、`11px`×4、`14px`×3、`12px`×3、`13px`×2、`9px`×1、`18px`×1、`1.5rem`×1、`em`×8、`calc()`×3、`inherit`×1。**新增字号前先跑一遍下面的命令**，避免"多一个 15px"。
2. **关闭钮 9 个、3 种实现**：`22px` 文本 ×5、`var(--fs-lg)` 文本 ×1（`ShortcutsModal:75` `.sc-close`）、`.btn-icon` + SVG ×3（`EntityDashboardModal` / `ResearchModal` / `SettingsModal`）。另有 **3 个弹窗压根没有关闭钮**（`InsightsModal`、`OnboardingModal` 用"跳过/完成"、`EntityAlertRuleModal` 只有"取消"）。
3. **`ResearchReportsTab` 的 4 处 `em` 是有意的**（Markdown 渲染按父级字号等比缩放），**不要"统一成 px"**，否则报告正文在不同字号下会失比例；同样 `ContentColumn` 的 `--reading-scale` 是"阅读字号"设置的作用点，别合并进界面字号。

```bash
# 复核：硬编码字号穷尽（37 处）
grep -rn "font-size:" src/components | grep -v "var(--fs" | wc -l
# 按值看分布
grep -rh "font-size:" src/components | grep -v "var(--fs" | sed 's/.*font-size:[[:space:]]*//' | sort | uniq -c | sort -rn
```

### 1.5 动效分布：**只有一个文件有 `@keyframes`**

| 文件 | `transition:` | `animation:` | `@keyframes` |
|---|---|---|---|
| **BaseModal** | 1 | 1 | **1**（`base-modal-in`） |
| EntityAlertRuleModal | 4 | 0 | 0 |
| FeedManageModal | 3 | 0 | 0 |
| SettingsModal / GlobalModal / ResearchModal / DigestModal | 2 | 0 | 0 |
| FeedAddModal / InsightsModal / OnboardingModal | 1 | 0 | 0 |
| **ShortcutsModal / RulesModal / EntityDashboardModal** | **0** | 0 | 0 |
| TagPicker（浮层） | 0 | 0 | 0 |

含义：**进出场动效 100% 在 `BaseModal` 一个文件里**。
"弹窗打开太生硬 / 太慢"的主战场只有 `BaseModal.vue` 的 `<style>`；而"弹窗内部元素不同步出现"（卡片 stagger、内容逐段淡入）要动各弹窗自己的 DOM，**13 个文件逐个加**，工作量与风险不是一个量级。

`ShortcutsModal` / `RulesModal` / `EntityDashboardModal` 目前**一条 transition 都没有**——hover 反馈是瞬时跳变。若设计稿要求"卡片 hover 有过渡"，这三个是从 0 开始加。

```bash
# 复核
for f in $(grep -rln "<BaseModal" src --include="*.vue"); do
  printf "%-32s t:%-2s a:%-2s kf:%s\n" "$(basename $f)" \
    "$(grep -c 'transition:' $f)" "$(grep -c 'animation:' $f)" "$(grep -c '@keyframes' $f)"
done
```

### 1.6 弹窗复用了哪些全局类（**这一节最重要**）

**判据**：模板 `class` 属性里出现过该 token，且该 token 在 `styles.css` 里有独立规则（全局类），或只在组件 `<style scoped>` 里有定义（局部类）。
**下面全是按 token 精确匹配统计的**（不是子串匹配——`form-input` 里的 `input`、`tag-chip` 里的 `chip` 都不算）。

**(A) 被弹窗复用、同时主界面也在用的全局类 → 改它必然连带主界面**

| 全局类 | 弹窗内 | 弹窗外 | `styles.css` 定义处 | 改它的后果 |
|---|---|---|---|---|
| **`.btn`** | **13 个文件 / 34 处** | **3 个文件 / 8 处**（`EntityGraph`、`GraphView`、`TagPicker`） | `:599`（高 32px、`font-size: var(--fs-sm)`） | 全应用按钮一起变 |
| `.btn-primary` | 8 / 12 | 1 / 1（`TagPicker`） | `:646` | 弹窗 8 个文件 + TagPicker |
| `.btn-sm` | 3 / 5 | 3 / 8（`EntityGraph`、`GraphView`、`TagPicker`） | `:679` | 尺寸档，6 个文件 |
| `.btn-secondary` | 4 / 4 | 1 / 1（`GraphView`） | `:632` | 5 个文件 |
| `.btn-default` | 4 / 4 | 1 / 1（`GraphView`） | `:631` | 5 个文件 |
| `.empty-state` | 4 个文件（Digest / ResearchTimeline / ResearchReports / ResearchEntities） | `ContentColumn`（**正文栏主空态**） | `:916-918` | 见 R9：主界面那份被同名 scoped 规则压住，**目前**只影响 4 个弹窗实例 |
| `.tag-chip` | **0**（不在弹窗内） | `ArticleListItem`、`TagPicker` | `:1012` | 与弹窗无关，别顺手改 |

**(B) 只在弹窗里被消费的全局类 → 改它们零主界面风险（本次最便宜的一块）**

| 类 | 使用者 | 定义处 |
|---|---|---|
| **`.tech-*`（26 个类）** | **只有 `EntityDashboardModal`**（24 个有全局定义；`tech-card-body` / `tech-group` / `tech-groups` / `tech-group-title` 是它自己的 scoped 定义） | `styles.css:1084-1460` 区间（1110 起是规则本体） |
| `.form-group` / `.form-label` / `.form-input` / `.form-hint` | 4–5 个 Settings tab | `:853-877` |
| `.setting` / `.setting-info` / `.save-hint` | 4 个 Settings tab | `:843-888` |
| `.tab-pane` | 5 个 Settings tab | `:840` |
| `.divider` | 3 个 Settings tab | `:880` |
| `.opml-actions` | 2 个 Settings tab | `:883` |
| `.extract-msg` | `ResearchReportsTab` / `ResearchTimelineTab` | `:906` |
| `.filter-bar` | `ResearchTimelineTab` | `:903` |
| `.btn-mini` / `.btn-danger` / `.btn-danger-text` | RulesModal / FeedManageModal / SettingsAutomationTab | `:684` / `:660` / `:672` |

> 上表里没有 `.hint` 与 `.btn-back`：前者只有嵌套规则 `.empty-state .hint`（`styles.css:918`），后者是 `ResearchReportsTab:431` 的**局部类**。**逐条核过才写进表里**——这类"看起来像全局、其实是局部"的类，改错方向会白改一轮。

> **`.tech-*` 是本次唯一"住在全局样式表里、实际只有一个消费者"的一大片**（36KB 的仪表盘样式全部服务于一个弹窗）。改它风险最低，但要先跑一次复核命令确认没有第二个使用者。

**(C) 三个容易误判的类**

- **`.btn-icon` 不是全局类**。`grep -rn "\.btn-icon" src` 只命中 3 个局部定义：`EntityDashboardModal:671`（嵌在 `.dash-modal-head` 下）、`SettingsModal:228`、`ResearchModal:210`。三份副本各写各的，改一处不影响另两处——**这是 R7 里"统一关闭钮"真正的坑**。
- **`EntityDashboardModal:14` 的注释写着"关闭钮复用全局 `.btn-icon`"**，而全局并没有这个类。这句注释会误导改动方向。
- **`BaseModal.vue:55` 的注释写着"side 布局自带面板右上角关闭钮"**，但模板里只有 `<slot />`，**没有这个按钮**；`--side-close-reserve: 48px`（`:336`）只有 `ResearchModal:228` 的注释提到并自行避让，`SettingsModal` 未引用。**"把关闭钮收进外壳"是新增 DOM + 迁移 9 个按钮的关闭事件，属于功能改动，要单独排期。**

```bash
# 复核 (A)(B)：某类在弹窗内 / 弹窗外各有多少文件
python3 - <<'PY'
import re,glob,os
modals=set(f for f in glob.glob('src/**/*.vue',recursive=True) if '<BaseModal' in open(f,encoding='utf-8').read())
sub={'BaseModal','MarketPanel','SettingsGeneralTab','SettingsAiTab','SettingsDataTab','SettingsObsidianTab',
     'SettingsAutomationTab','ResearchTimelineTab','ResearchReportsTab','ResearchEntitiesTab'}
tree={f for f in glob.glob('src/**/*.vue',recursive=True) if os.path.basename(f)[:-4] in sub}|modals
for cls in ['btn','btn-primary','btn-sm','empty-state','tag-chip']:
    ins=out=0
    for f in glob.glob('src/**/*.vue',recursive=True):
        tpl=open(f,encoding='utf-8').read().split('<template>',1)[-1].split('</template>',1)[0]
        hit=any(cls in re.sub(r'\{[^}]*\}','',m.group(1)).split() for m in re.finditer(r'class="([^"]*)"',tpl))
        if hit: ins+= f in tree; out+= f not in tree
    print(f"{cls:<14} 弹窗内 {ins} 文件 / 弹窗外 {out} 文件")
PY
grep -o "\.tech-[a-z-]*" src/styles.css | sort -u | wc -l     # 26
grep -rln "tech-" src --include="*.vue"                        # 只有 EntityDashboardModal + MarketPanel
```

### 1.7 字重：**没有 token**

全仓分布：`600` ×67、`500` ×27、`700` ×17、`400` ×4、**`650` ×3**。没有 `--fw-*`，所以"标题统一加粗一档"必须逐处改。

`650` 三处：`OnboardingModal.vue:155`（`.onb-title`）、`styles.css:1134`（`.tech-group-title`）、`styles.css:1199`（`.tech-card-title`）。后两处都在弹窗内（实体仪表盘）。风险见 R5。

### 1.8 两个"隐身耦合"：zoom 与 z-index

**(a) 弹窗不在 `zoom` 容器内**（影响所有字号判断）

```
styles.css:433   body { font-size: var(--fs-md) }            → 14px 基准
styles.css:448   .app { zoom: var(--ui-zoom, 1) }            → 0.9 / 1 / 1.1 / 1.2
appearance.ts:31-36  UI_ZOOM 档位表；:154 写入 documentElement
BaseModal.vue:10 <Teleport to="body">                        → 弹窗在 .app 之外 ⇒ 不受 zoom 影响
```

后果：用户把「设置 → 常规 → 界面字体大小」调到"大"（1.1），**主界面放大 10%，弹窗一点不变**。
对照：`ContextMenu.vue:44-47`、`AppTooltip.vue:52-56`、`SelectCapsule.vue:265-286` 三个同样 Teleport 到 body 的浮层，**都在注释里写明"位于 zoom 容器之外"并做了坐标换算**；**弹窗和 `AppSelect` 这两处从未处理过**（见 R1.1）。

**(b) z-index 全景**（改弹窗层级前必看）

| 层 | 值 | 位置 |
|---|---|---|
| 图片灯箱 | 2000 | `ContentColumn.vue:959` |
| **全局确认框** | **9999** | `BaseModal.vue:261`（`.overlay.global`） |
| Tooltip / Toast | 1100 | `AppTooltip.vue:172` / `ToastHost.vue:59` |
| **弹窗遮罩** | **1000** | `BaseModal.vue:241` |
| **`AppSelect` 下拉面板** | **1000** | `AppSelect.vue:430`（同值，靠 DOM 顺序取胜） |
| ContextMenu / FeedPanel 浮层 | 950 | `ContextMenu.vue:454` / `FeedPanel.vue:1458` |
| 命令面板 | 900 | `CommandPalette.vue:703` |
| 标题栏 | 500 | `TitleBar.vue:258` |

`AppSelect` 的下拉**靠 DOM 顺序**（Teleport 后插在 body 末尾）压过弹窗遮罩，而不是靠数值。风险见 R6。

---

## 2. 改法风险表（设计稿方案 → 代码里的实际后果）

### R1 · 在 `.base-modal` 作用域覆盖 `--fs-*`（最常见、也最容易出暗坑）

```css
/* BaseModal.vue <style scoped> 里加 */
.base-modal {
  --fs-xs: 12px;  --fs-sm: 14px;  --fs-md: 15px;
  --fs-base: 15px; --fs-lg: 17px; --fs-xl: 20px; --fs-2xl: 24px;
}
```

**能做到**：一次覆盖 **148 处** = 12 个宿主 + 8 个 tab + `MarketPanel` 共 **125 处 `.vue` 声明** + `styles.css` 里那 **23 处"只被弹窗消费的全局规则"**（`.tech-*` / `.form-*` / `.setting*`）——后者能生效是因为覆写的是**变量**，规则本身不动。
**唯一盖不到的是 `AppSelect` 的 3 处**（面板不在子树里，见下条）。

**做不到 / 会坏的地方（按严重度）**：

1. **`AppSelect` 的下拉面板不会跟着变**。它在 `AppSelect.vue:46` `Teleport to="body"`，**不在 `.base-modal` 子树里** ⇒ 触发器按新字号渲染、下拉列表仍是全局字号。`AppSelect` 出现在 **8 个弹窗内文件**（`FeedAddModal` / `FeedManageModal` / `RulesModal` / 5 个 tab / `ResearchReportsTab` / `ResearchTimelineTab` / `ResearchEntitiesTab`）和主界面 `GraphView`，用户一眼能看出不一致。
   - 修法：把同一组覆盖值也挂到 `.as-panel`（它也在 body 下，需要另写一条**非 scoped** 规则，或改成用 `:global()`）；**不要**为了字号在 `BaseModal` 里再套一层 Teleport——那会把焦点陷阱与 `offsetParent` 过滤（`BaseModal.vue:119`）一起改掉。
2. **`AppSelect` 的定位没有 zoom 换算**。它用 `getBoundingClientRect()`（`AppSelect.vue:204`）算 `position: fixed` 坐标；同类浮层里**只有它没写除以 zoom 的换算**。今天没出事是因为触发器和面板分处两个坐标系、rect 恰好是视觉像素；**一旦给 `.base-modal` 或 `.overlay` 加 `zoom` / `transform`，这个"恰好"就破了**，表现为下拉面板错位。→ 真机 4 档 zoom 逐档验（§3.4 第 1 项）。
3. **`ResearchReportsTab` 的 `.md` 用 `em` 缩放**（`1.35em / 1.15em / 1.02em / 0.9em`）：父级字号一变，报告正文等比变化。若设计稿只想放大"界面文字"、不想放大"报告正文"，这里要单独锁死。
4. **`--fs-md` / `--fs-base` 当前值相同（都 14px）**，两个语义被当成一个用（`body` 用 `--fs-md`、多处正文用 `--fs-base`）。覆盖时若给它们不同值，会**第一次**把这两个语义分开——好事，但要逐个弹窗核对，它会暴露此前所有"随手写 `--fs-base`"的地方。
5. **作用域覆盖会让"同一个全局类"在弹窗内外长得不一样**：`styles.css:849` 的 `.setting-info h4 { font-size: var(--fs-md) }` 在弹窗内跟着覆盖值走、在主界面（若将来复用）走全局值。这不是 bug，但 review 时会被当成 bug 争论一次，先写进 PR 描述。

**建议**：若设计稿只是"整体大一号"，**只覆盖真正需要的 2–3 个 token**（如 `--fs-sm` + `--fs-md`），并**必须**连同 R1.1 一起处理 `AppSelect`；7 个一起改等于把影响面拉到最大，收益却一样。

---

### R2 · 加退出动画（现状：**进场有位移、退场只有淡出**，面板常驻 DOM）

现状（`BaseModal.vue`，行号见 §1.5）：

```css
.overlay { opacity: 0; visibility: hidden;
  transition: opacity var(--dur) var(--ease), visibility var(--dur) var(--ease); }
.overlay.show { opacity: 1; visibility: visible; }
.overlay.show .base-modal { animation: base-modal-in var(--dur) var(--ease); }
/* @keyframes base-modal-in: translateY(8px) + opacity 0 → 1 */
```

**先看清现状，别修错东西**：

- **进场的位移只挂在 `.show` 上**，关闭时 `class` 一去掉，`animation` 立即消失 ⇒ 面板没有"向下退场"的位移。
- **但整体并不是瞬消**：`.overlay` 的 `opacity` 会过渡 220ms，面板作为子元素跟着淡出。
  所以用户体验到的是**"打开时向上滑入 + 关闭时原地淡出"的不对称**——这正是"交互不流畅"的典型来源，也是这类改版最该先修的一处。
- **`visibility` 参与过渡时是离散插值**：只要有一端是 `visible`，整个过渡期间计算值都是 `visible`。所以 `BaseModal.vue:249` 的注释「visibility 一起参与过渡：淡出期间就不会再挡住底层的点击」**与规范相反**——淡出的这 220ms 里遮罩仍然接收点击，**用户关掉弹窗后立刻点它后面东西，会有一瞬间点不动**。这条不是观感问题，是可复现的操作卡顿（30 秒可验：关闭弹窗后立即点列表项，若第一次无效而第二次正常，就是它）。

**可直接照做的方案（纯 CSS，无需 JS）**：把"进场用 `animation`、出场无"改成"**两个方向都用 `transition`**"，把 `visibility` 的翻转推迟到淡出结束，并用 `pointer-events` 让遮罩立刻放行点击。

```css
/* 1) 面板：两个方向都走 transition。位移只挂在面板上，遮罩只管 fade */
.base-modal {
  opacity: 0;
  transform: translateY(10px);
  transition: opacity var(--dur) var(--ease), transform var(--dur) var(--ease);
}
.overlay.show .base-modal { opacity: 1; transform: none; }

/* 2) 遮罩：关 → 淡完再 hidden（delay = --dur）；开 → 立刻 visible（delay 0） */
.overlay {
  opacity: 0; visibility: hidden; pointer-events: none;
  transition: opacity var(--dur) var(--ease), visibility 0s linear var(--dur);
}
.overlay.show {
  opacity: 1; visibility: visible; pointer-events: auto;
  transition: opacity var(--dur) var(--ease), visibility 0s linear 0s;
}
/* 3) 删掉 :290-292 的 .overlay.show .base-modal { animation: base-modal-in … }
      与 :293-302 的 @keyframes base-modal-in —— 留着会和新 transition 抢 opacity/transform，
      表现为"过渡走到一半被动画覆盖"的闪烁 */
```

五个要点，少一个就会出观感问题：

- **`visibility 0s linear var(--dur)`**：延到淡出结束才 `hidden`；不这么写，面板在淡出的第一帧就被隐藏（"改了没效果"的最常见原因）。开门方向那条的 delay 必须是 `0s`，否则打开时先空白 220ms。
- **`pointer-events: none` 在非 `.show` 态**：这是修"关闭后 220ms 点不动"的唯一手段（`visibility` 帮不上，理由见上）。
- **`animation` 与 `transition` 不要同时作用在 `opacity`/`transform` 上**：`animation` 优先级更高，会让 transition 完全失效——"我加了 transition 但没生效"的经典现象。
- **别在两层都挂 `opacity`**：遮罩与面板各做一次淡入，视觉上是两个 opacity 相乘（50% × 50% = 25%），淡得比设计稿快。要么面板只留 `transform`，要么遮罩只留 `visibility`。
- **唯一会让这套方案失效的改动**：把宿主写成 `<BaseModal v-if="open">`。当前 **12 个宿主全部传 `:open`**（`grep -n "<BaseModal" src/components/*.vue` 可证），组件常驻 ⇒ 退场有机会播。若将来有人改成 `v-if`，组件立刻销毁，任何 CSS 退场都不生效。

**顺带一条（必须一起做）**：`BaseModal` **不响应 `prefers-reduced-motion`**（全仓 6 个组件写了 `@media (prefers-reduced-motion)`：`CommandPalette`/`FeedPanel`/`ToastHost`/`ArticleChatPanel`/`SelectCapsule`/`ArticleSummaryCard`；另有 3 处 JS `matchMedia`；弹窗外壳不在其中）。加了动效就要一起加：

```css
@media (prefers-reduced-motion: reduce) {
  .overlay, .base-modal { transition-duration: 0.01ms !important; }
}
```

**单段退场不需要任何 JS**：撤掉 `.show` 就是一次"有前值"的属性变化，`transition` 必然触发（这也是上面方案成立的原因）。**只有**"内容先微缩、再整体淡出"这类**两段式**才需要 `transition-delay` 叠加，**那才需要**在 `BaseModal` 里加 `closing` 状态并在 `transitionend` 后清理——**不建议**为这点观感差引入状态机：面板常驻、`watch(open)` 已有激活/去激活逻辑（`BaseModal.vue:230`），多一个状态就多一处"快速连点 / 跨零点"的边界。

**与 `modal-interaction-review.md` §3.2.2 的对齐**（两份文档同口径，别各写一套）：时序与曲线用那份定义的 `--m-in: 200ms` / `--m-out: 120ms`（挂在 `.overlay` 上，**不重定义 `--dur` / `--ease`**），上面片段里的 `var(--dur) var(--ease)` 相应换成 `var(--m-in)/var(--m-out)`。注意 `--m-*` 挂在 `.overlay` 上能继承进 `.base-modal`（它是 `.overlay` 的子元素），**但盖不到 Teleport 到 body 的 `AppSelect` 面板**——与 R1.1 是同一个坑。

**一个必须写进 review 的埋雷点**：`.base-modal` 目前**没有任何 `transform`**（只有 `:296` / `:300` 在 `@keyframes` 里），居中靠 `.overlay` 的 flex（`:242-244`）。所以基础态写 `translateY(6px)` 是安全的；**但将来若有人把居中改成 `transform: translate(-50%, -50%)`，退场位移会被整体替换，面板会跳到左上角**。规则一条：**`.base-modal` 上新增 `transform` 必须与退场用的 `translateY(...)` 合写在同一处声明里**，不能另起一条覆盖。

---

### R3 · `backdrop-filter: blur(28px)` + 动效在低配机上的实际开销

现状是**两层玻璃**：遮罩 `blur(10px) saturate(0.92)`（`BaseModal.vue:247`）+ 面板 `blur(28px) saturate(1.7)`（`:275`）。
`backdrop-filter` 要对**背后内容**实时采样模糊；面板在进场时 `translateY` 位移 ⇒ **每帧重新采样整个背景**（遮罩铺满屏幕 ⇒ 成本 ≈ 全屏 × 2 层 × 帧数）。

- **"只用 transform/opacity 就不用担心性能"是错的**。它避开了 layout/paint 重排，但 `backdrop-filter` 的采样发生在**合成阶段**，照样每帧重算。
- 真正有效的手段，按性价比排序：
  1. **缩小模糊面积**：面板的 `blur(28px)` 只作用于面板盒子（≤960×720），可控；**遮罩的 `blur(10px)` 是全屏的**，是主要开销。卡就先降遮罩（8px 或去掉，靠 `--overlay-scrim` 压暗即可）。
  2. **别让玻璃动**：进场位移放到内层包裹（`.base-modal > .panel-inner`），外层玻璃不动 ⇒ 采样结果可复用。这条与 R2 的"transform 挂面板"互斥，**二选一并在 PR 里写明选了哪个**。
  3. 动画期间不要改 `filter` / `saturate` 的值。
  4. 不要加 `will-change: backdrop-filter`（强制常驻图层，内存涨、滚动更卡）。
- **判定标准（可照抄进验收）**：1366×768 + 集显机型上，连续开关弹窗 10 次，**掉帧（肉眼可见的停顿）超过 2 次即视为不合格**；此时按上面第 1 条降遮罩模糊，再测。
- 注意 `backdrop-filter` 会**创建包含块**（`BaseModal.vue:6-9` 的注释记着这件事）：面板内任何 `position: fixed` 后代都会被"关"在面板里。**新增浮层一律 Teleport 到 body，不要用面板内 fixed**。

---

### R4 · 插入新的头部/说明段 → 结构伪类问题（**弹窗里已经有 2 处，只是不在组件文件里**）

先纠正一个容易得到的错误结论：**只在 13 个弹窗与 9 个子组件（5 个 Settings tab + 3 个 Research tab + `MarketPanel`）的 `<style>` 里 grep，结果是 0 处**——

```bash
grep -rn ":first-child\|:nth-child\|:last-child\|:nth-of-type" \
  src/components/{BaseModal,SettingsModal,ResearchModal,EntityDashboardModal,DigestModal,InsightsModal,RulesModal,FeedAddModal,FeedManageModal,EntityAlertRuleModal,GlobalModal,ShortcutsModal,OnboardingModal}.vue \
  src/components/Settings*Tab.vue src/components/Research*Tab.vue src/components/MarketPanel.vue
# 无输出 —— 但这不代表安全
```

**但全局样式表里有 2 条结构化规则，就落在弹窗 DOM 里**：

| 规则 | 位置 | 落在哪个弹窗 | 会被什么动作打破 |
|---|---|---|---|
| `.setting:last-of-type { border-bottom: none }` | `styles.css:848` | `SettingsModal` 的 5 个 tab（`.setting` 是设置项容器） | **在任意设置组末尾追加一个 `.setting`**（例如新增"危险区 / 关于"）⇒ "最后一项去边框"落到新项上，原来的最后一项突然长出一条分隔线 |
| `.tech-tl-item:last-child::after { … }` | `styles.css:1307` | `EntityDashboardModal` 的时间轴（`::after` 画向下连接线，末项不画） | **给时间轴补一个"查看全部"尾项** ⇒ 连接线断在倒数第二项，末项悬空；或插一个新项在中间 ⇒ 连接线接到最后一个上 |

这两处正是 `.rv-cluster:first-child` 那次的同一机制：选择器依赖"我在兄弟里的位置"，插入一个元素就**静默套错对象**，不报错、不红、CI 也过。
（补充：`grep -rn "rv-cluster" src` 现在**无输出**，那次事故的类名已不在代码里，只能按机制排查。）

**回答"会不会重现"**：**已经在弹窗里了（2 处），只是不在你以为的文件里。** 而且本次改版的高危动作恰好就是"加一行说明 / 副标题 / 尾项"：

| 设计稿动作 | 会破坏什么 | 安全写法 |
|---|---|---|
| 给设置面板加"危险区 / 关于"分组 | `.setting:last-of-type`（`styles.css:848`） | 新分组用不同类名（`.setting-danger`），或改成 `.setting:last-child` 前先确认 DOM 顺序 |
| 给实体仪表盘时间轴加"查看全部" | `.tech-tl-item:last-child::after`（`styles.css:1307`） | 尾项用独立类名（`.tech-tl-more`），不参与 `--tl-item` 样式 |
| 在列表前插"说明段" | 任何 `:first-child`（当前弹窗内 0 处，但你可能正打算加） | 列表套独立容器，让说明段与列表**不同父级**；或给首项显式类名 |
| 在 `.m-body` 顶部插副标题 | 顶部 padding 预留（`ResearchModal.vue:228` 明写要与 `--side-close-reserve` 对齐） | 改头部高度后，两个 side 宿主的 `padding-top` 一起改 |

**一句话规则**：本次改版**新增样式一律不用结构伪类**；要"首项/末项特殊"，用显式类名或容器分层。这条比任何 review 都便宜。

---

### R5 · `font-weight: 650` 在本机字体上会怎么渲染

字体栈（`styles.css:280`，全应用唯一定义处；`appearance.ts:61` 有同一份回退串）：

```css
--font: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
--ui-font: var(--font);   /* 可被「界面字体」设置覆盖成任意系统字体，appearance.ts:159-165 */
body { font-family: var(--ui-font); font-size: var(--fs-md); }
```

> 注：`src-tauri/src/fonts.rs` **不定义栈**，它只负责枚举本机已安装字体，供"界面字体"下拉用。字体栈的真源就是上面两处。

栈里**没有中文族名**，中文全部落到各平台默认 CJK：macOS = PingFang SC，Windows = Microsoft YaHei，Linux = Noto Sans CJK。

CSS 字重匹配规则：**目标值 > 500 时，先向上找 ≥ 目标的可用字重，找不到再向下找**。于是：

| 目标 | macOS PingFang SC | Windows YaHei | 结果 |
|---|---|---|---|
| `600` | 有 600 → 600 | 无 600 → 向上取 **700** | 跨平台不一致 |
| **`650`** | 无 650 → 向上取 **700** | 无 650 → **700** | **中文一律 700（= bold）** |
| `650`（拉丁部分） | SF Pro 是可变字体 → 可能真是 650 | Segoe UI Variable（Win11）→ 650；旧版静态字重 → 700 | 视系统而定 |

**结论**：中文界面里 `650` 与 `700` **渲染成同一档**，设计稿想要的"介于半粗和粗之间"的层次不存在；中英混排时（如"欢迎使用 ReadFlow"）拉丁部分是 650、中文部分是 700，**同一行两种粗细**。

**验证方法（比争论快）**：devtools 里选中该元素 → Computed 面板看到的是**声明值 `650`**（会骗人）→ 切到 **Rendered Fonts** 看实际使用的族与字重，那才是真相。

**给开发的取舍**：中文里只有 **500 / 600 / 700 三个真实档**；`650` 现存 3 处、改动面极小，**不建议本次新增**。`600` 在 Windows 上也会变 700，这是全应用 67 处的既有事实，不在本次范围，但设计评审时不要把"600 vs 700 的层次"当成已经存在的前提。

---

### R6 · 抬高弹窗 `z-index`（"让它更浮"）

`AppSelect` 下拉面板是 **1000**，与弹窗遮罩**同值**，靠"Teleport 后插在 body 末尾"的 DOM 顺序压过遮罩。

- **若把 `.overlay` 提到 1001 以上**：`FeedAddModal` / `FeedManageModal` / `RulesModal` / 5 个 Settings tab / 3 个 Research tab 里的所有下拉框**都会掉到遮罩后面**——表现为"点开下拉，列表看不见 / 点不到"，而且**只在弹窗里复现**，主界面正常，极难定位。
- **若提到 1100 以上**：`AppTooltip`(1100) 与 `ToastHost`(1100) 会被遮罩盖住 ⇒ 弹窗内的提示气泡、任务完成 toast 全部不可见。
- **若降低上限（把 `.overlay.global` 从 9999 往下调）**：弹窗内触发的全局确认框会被它所在的弹窗盖住，`GlobalModal` 的"先让路"逻辑（`BaseModal.vue:43-45`，判断在 `onKeydown:155`）也随之失效。

**建议：不要动 `z-index` 数值。** 要"更浮"，用 `--shadow-pop`（`styles.css:277` / 暗色 `:420`）、`--glass-hair` 描边、`--overlay-scrim` 深度（`:34` / 暗色 `:336`）与模糊强度表达层次——四项都是纯视觉、零层级风险。

---

### R7 · 统一关闭钮（9 个按钮 / 3 种实现 / 3 个弹窗没有）

| 形态 | 文件 | 尺寸来源 |
|---|---|---|
| `×` 文本 | `DigestModal` `.m-close` / `FeedAddModal` `.add-close` / `FeedManageModal` `.manage-close` / `RulesModal` `.manage-close` / `GlobalModal` `.modal-close-btn` | **硬编码 22px** ×5 |
| `×` 文本 | `ShortcutsModal` `.sc-close`（`:72-79`） | `var(--fs-lg)`（16px） |
| SVG | `EntityDashboardModal:21` / `ResearchModal:29` / `SettingsModal:28`（`.btn-icon`） | **3 份局部 `.btn-icon` 副本**（`:671` / `:210` / `:228`），不是全局类 |
| 无按钮 | `InsightsModal` / `OnboardingModal`（走"跳过/完成"）/ `EntityAlertRuleModal`（只有"取消"） | — |

**两点要注意**：

1. **只做视觉统一（不改结构）**：把 5 处 `22px` 换成 `var(--fs-2xl)` 或新建语义 token（如 `--icon-close-size`），`.sc-close` 与 3 份 `.btn-icon` 各自对齐。**改动 9 行以内，零结构风险**，是性价比最高的一步。
2. **要"收进外壳统一"则是功能改动**：`BaseModal` 模板里没有关闭钮（见 §1.6(C)），意味着新增 DOM + 迁移 9 个按钮的关闭事件 + 重测焦点陷阱（`querySelectorAll` 的 DOM 顺序与 `offsetParent !== null` 过滤，`BaseModal.vue:119`）。**单独排期，别混在视觉改版里。**

---

### R8 · 全局 `button` 已带 transition（会被"局部改"覆盖）

`styles.css:567-573`：

```css
button { transition: background …, color …, border-color …, box-shadow …, transform … var(--dur) var(--ease); }
button:not(:disabled):active { transform: scale(0.96); }
```

含义：**全应用每个按钮的按压反馈都来自这条全局规则**。若设计稿要求"弹窗按钮 hover 上浮 / 按下回弹更明显"：

- 写在全局 `button` 上 ⇒ **42 处 `.btn` + 13 处 `.btn-primary` / 13 处 `.btn-sm` 等修饰类 + 所有原生按钮一起变**（含主界面）；
- 写在弹窗作用域内 ⇒ **必须写成 `.base-modal :deep(button) { … }`**，直接写 `.base-modal button` **不会生效**：`<style scoped>` 会把属性选择器加在**最后一个复合选择器**上，编译成 `.base-modal button[data-v-<BaseModal>]`，而插槽里的按钮带的是**宿主组件的** `data-v`，两者对不上（见 R10）。另外**必须写全 6 个属性**——只写 `transform` 会把其余 5 个过渡覆盖成"无过渡"，观感反而更硬。

---

### R9 · `.empty-state` 的双重定义（"改全局类会不会打到主界面"的真实答案）

`.empty-state` 有 5 个使用者：4 个弹窗内（`DigestModal` / `ResearchTimelineTab` / `ResearchReportsTab` / `ResearchEntitiesTab`）+ **主界面正文栏 `ContentColumn.vue:150`**（"接下来读点什么？"那一屏）。

但它**被定义了两遍**：全局 `styles.css:916-918` 一份，`ContentColumn` 的 scoped 里 `:973-1000` 又一份（`.empty-state` / `.empty-title` / `.empty-hint` 全覆盖）。

- **现状**：scoped 版本带 `[data-v-*]` 属性，特异性高于全局类 ⇒ 主界面实际吃的是 scoped 那份，**改全局 `.empty-state` 只影响 4 个弹窗实例**。
- **但这是巧合，不是设计**：哪天主界面空态新增一个不带 scoped 定义的元素，它就会落回全局规则上，跟着弹窗的字号一起变。
- **动作指引**：想改弹窗空态，**改全局 `.empty-state` 是对的**（影响面 4 处）；但要在 PR 里写一句"主界面空态由 scoped 覆盖，本次不受影响"，否则 review 会卡在这里。

---

### R10 · `<style scoped>` 与插槽内容：变量能继承进去，选择器选不中

这是本次改版最容易"写了没效果"的一条，也是 R1 与 R8 成立/不成立的分界。

`BaseModal` 渲染的是 `<div class="base-modal"><slot /></div>`。插槽内容是**宿主组件**编译的，元素上带的是**宿主的** `data-v-xxx`，不是 `BaseModal` 的。而 `<style scoped>` 会把 `[data-v-<BaseModal>]` 加到每条规则**最后一个复合选择器**上。于是：

| 写法 | 结果 |
|---|---|
| `.base-modal { --fs-sm: 14px }` | ✅ **生效**。自定义属性靠 **DOM 继承**传播，与属性选择器无关，`<slot>` 内容照样吃到 |
| `.base-modal .m-title { … }` | ❌ **不生效**（编译成 `.base-modal .m-title[data-v-B]`，元素的 `data-v` 是宿主的） |
| `.base-modal :deep(.m-title) { … }` | ✅ 生效 |
| `.overlay.show .base-modal { transform: … }` | ✅ 生效（两个元素都是 `BaseModal` 自己渲染的） |

**所以**：
- 想"一次性改变弹窗内的字号/间距"，唯一可行的作用域手段是**覆盖变量**（R1），不是写后代选择器；
- 想改弹窗内的**具体选择器**（如 R8 的 `button`、R7 的关闭钮），必须 `:deep()`，并且要意识到 `:deep()` 会穿透**所有**层级（包括你不想动的子组件内部），影响面按"整棵子树"算，不按"你写的那一层"算；
- 若把新规则写在**宿主组件**（如 `SettingsModal`）的 scoped 里，就不需要 `:deep()`——但那样只作用于该弹窗，13 个弹窗要写 13 遍。**选择"一处 + `:deep()`"还是"13 处 + 无 `:deep()`"，是本轮唯一的架构取舍，请在 PR 里显式写明选了哪个。**

---

## 3. 验收手段（改完逐条自查）

### 3.1 `npm run typecheck` 能查出什么、查不出什么

```bash
npm run typecheck      # = vue-tsc --noEmit
```

**当前基线：0 错误（exit 0，无输出）**，2026-10-30 实跑。**所以新增任何一条错误都是你引入的**，不要用"本来就有红"搪塞。

| 能查 | 查不出（**必须靠别的办法**） |
|---|---|
| TS 类型错误、strict 下的空值问题 | **`.vue` 导入路径写错**（见下） |
| 未使用的局部变量 / 参数 | CSS 里写了不存在的类名 |
| `props` 类型与调用处不匹配 | `var(--未定义)`（整条声明静默作废） |
| 模板里引用未定义的 `ref` / 方法 | 类名拼写错误（样式不生效但不报错） |
| | 选择器层级写错（`:deep()` 边界） |

**为什么 `.vue` 路径写错查不出来**：`src/vite-env.d.ts` 里有全局垫片

```ts
declare module '*.vue' { const component: DefineComponent<{}, {}, any>; export default component }
```

任何 `.vue` 说明符都会命中这个通配声明 ⇒ `import X from './Fooo.vue'`（拼错）**类型检查通过**，只有 Vite 运行时才报错。**新增/重命名文件后不要只信 typecheck**，用 3.2 的 grep 兜。

### 3.2 死类检查 与 未定义 token 检查

**(a) 死类（模板用了、样式里没定义）**

```bash
# 单文件：模板类名 与 <style> 定义类名 做差集
python3 - <<'PY'
import re
f='src/components/SettingsModal.vue'; s=open(f).read()
tpl=s.split('<template>',1)[1].split('</template>',1)[0]
css=s.split('<style',1)[1]
used=set()
for m in re.finditer(r'class="([^"]*)"',tpl):
    used|={c for c in re.sub(r'\{[^}]*\}','',m.group(1)).split() if re.fullmatch(r'[A-Za-z][\w-]*',c)}
defined=set(re.findall(r'\.([A-Za-z][\w-]*)',css))
print("模板用了但组件样式没定义:", sorted(used-defined))
PY
```

> 全局类（`.btn` / `.tech-*` / `.form-input` …）本来就不在组件样式里，会被这个差集**误报**。判据是"**既不在组件样式、也不在 `styles.css`**"：

```bash
grep -n "\.名字\b" src/styles.css          # 有输出 = 全局类，不算死类
```

> 本次实际跑过：`EntityDashboardModal` 里的 `.badge`、`GlobalModal` 里的 `.modal-title` / `.modal-body` 都是**局部定义**，不是死类；`.btn-icon` 三份局部副本同理。**没有发现真正的死类**（早期怀疑的 `.input` / `.chip` / `.kbd` 全是子串误报：`form-input`、`tag-chip`、`sc-kbd`）。

**(b) 未定义 token（`var(--x)` 里 `--x` 没人定义 ⇒ 整条声明静默作废，不报错）**

```bash
python3 - <<'PY'
import re, glob
defined={m.group(1) for m in re.finditer(r'(?m)^\s*(--[a-z0-9-]+)\s*:', open('src/styles.css').read())}
used={}
for f in glob.glob('src/**/*.vue',recursive=True)+glob.glob('src/**/*.css',recursive=True):
    for m in re.finditer(r'var\((--[a-z0-9-]+)', open(f).read()):
        used.setdefault(m.group(1),set()).add(f)
missing={k:v for k,v in used.items() if k not in defined}
for k,v in sorted(missing.items()): print(k,'未定义，引用处:',', '.join(sorted(v)))
print('未定义 token 数:',len(missing))
PY
```

> **这条必须做**：`.content-body h1` 曾引用从未定义的 `--fs-2xl`，导致正文 h1 静默继承 14px（`styles.css:288-290` 记着这段往事）。它是"我明明写了 22px 却不生效"的唯一解释。
> 若你在 `.base-modal` 里**新定义** `--fs-*` 覆盖值，**不会**被这个脚本报出来——它只查"引用了但没定义"，不查"定义在哪个作用域"。作用域是否正确只能靠真机看。

### 3.3 不要跑 `npm run build:frontend`

```bash
npm run build      # = typecheck + build:frontend
```

`build:frontend`（`vite build`）在本沙箱里因 undici 的 WASM 初始化 OOM **跑不完**，会挂在打包阶段而不是报代码错误。**验收只用 `npm run typecheck` + 真机 `npm run dev`**；把 build 失败当代码问题会白等很久。需要产物时由 CI 或本机环境执行。

### 3.4 人工检查清单（真机，逐条打勾）

**必备四项（每次改动都过一遍）**

1. **界面字体大小四档**：设置 → 常规 → 界面字体大小 = 小(0.9) / 中(1) / 大(1.1) / 特大(1.2)，**每档打开每个弹窗**。
   - 判据 A：主界面与弹窗的相对字号关系是否合理（弹窗不随 zoom，见 §1.8a）。
   - 判据 B（引入作用域覆盖或缩放后必查）：**弹窗内任一 `AppSelect` 的下拉面板是否与触发器左边缘对齐、字号一致**（R1.1 / R1.2）。这一项最容易在"只改了字号、没管 Teleport"时翻车。
2. **暗色主题**：`--glass-strong`（浅 `styles.css:142` / 暗 `:359`）与 `--overlay-scrim`（`0.4` / `0.6`）是两套值。玻璃与阴影必须暗色单独看——浅色下的"轻"在暗色里常变成"糊"。
3. **减弱动效**：系统开启"减少动态效果"（macOS 辅助功能 / Windows 关闭动画效果）后开关弹窗。
   - 判据：没有位移、没有缩放；仅允许瞬时出现。`BaseModal` 目前**没有**这条媒体查询（R2 末尾）。
4. **键盘走一遍**：Tab 进弹窗 → Tab 循环不出面板 → Esc 关闭 → 焦点回到打开它的按钮。
   - 这是 `BaseModal` 已有能力（可聚焦集合 `focusables :114-120`、Esc 与 Tab 循环 `onKeydown :149-201`（让路判断 `:155`）、激活/去激活 `:203-228`、`watch(open) :230`）。**若你把关闭钮移进外壳或改了 DOM 顺序，必须重测**：焦点陷阱依赖 `querySelectorAll` 的 DOM 顺序与 `offsetParent !== null`（`:119`），隐藏元素（如未激活 tab）被排除的逻辑一旦被新结构打断，焦点会跳进看不见的控件。

**按改动类型追加**

| 你改了什么 | 额外必看 |
|---|---|
| 字号 / 行高 | 最长文案的弹窗（`EntityAlertRuleModal` 规则说明、`SettingsAutomationTab` 表单）是否换行溢出；`SettingsModal` / `ResearchModal` 是**固定高** `min(80vh,720px)`，字变大最先在这里撑破 |
| 动效 | **关闭后立刻点遮罩下方的东西**——必须 0 延迟响应（R2 的 `pointer-events` 项）；再快速连点"开→关→开"三次，第二次打开是否正常播放；另确认**没有再给 `.base-modal` 加第二条 `transform`**（会顶掉退场位移，见 R2 末尾） |
| 内边距 / 头部高度 | `SettingsModal` 与 `ResearchModal` 的 `.m-body` 顶部预留是否仍避开关闭钮（R4 末行） |
| 玻璃 / 模糊 | 1366×768 + 集显上连续开关 10 次，掉帧 ≤2 次（R3 判定标准）；文字在 `saturate(1.7)` 下是否仍够对比 |
| 下拉框 / 开关 | 弹窗内 `AppSelect` / `AppSwitch`：字号一致（R1.1）、下拉不被遮罩盖住（R6） |
| 设置面板分组 | 末尾是否新增了 `.setting` ⇒ 分隔线位置（R4） |
| 实体仪表盘时间轴 | 是否新增 `.tech-tl-item` 尾项 ⇒ 连接线（R4） |

---

## 4. 边界：这次**不做**什么

- **不做样式重构**：不把 151 处字号声明收敛成 token、不把 9 个关闭钮合并成一个组件、不动 `styles.css` 的 `.btn` 族结构。这些会把"观感改进"变成"影响主界面的大改"，改动不可控。
- **不改 `--fs-*` 的全局值**（会连带主界面 81 处 + 全局 8 处）。
- **不改 `z-index` 数值**（R6）。
- **不新增非标准字重 650**（R5）。
- **不给 `.overlay` / `.base-modal` 加 `zoom`/`transform` 替代"字号调整"**——除非连 `AppSelect` 的定位换算一起做（R1.2）。
- **不把 `TagPicker` 当弹窗改**（§1.1）。

---

## 5. 第二轮核查补充（fe-engineer 追加，2026-10-30）

> 同一棵树上独立复算后，**只补三处本文此前没有覆盖的东西**，其余结论一致，不重复。

### 5.1 【硬阻塞·无对应条目】放大字号会被面板裁掉，6 个弹窗没有内部滚动容器

`.base-modal` 同时有 **`max-height: 86vh`（`BaseModal.vue:271`）** 和 **`overflow: hidden`（`:285`）**。这不是"溢出会出滚动条"，而是"**超出的部分直接消失，且没有滚动条可以够到**"。面板本身不滚，只有自带内部滚动区的宿主才安全。

| 弹窗 | 内部滚动区（`overflow-y:auto` / `overflow:auto`） | 放大字号后的暴露风险 |
|---|---:|---|
| `DigestModal` | 1 | 安全 |
| `EntityDashboardModal` | 1 | 安全（`:675-678` 注释已写明"面板 86vh 由 BaseModal 约束，本区 `flex:1` + overflow 让长内容在弹窗内滚动"） |
| `RulesModal` / `ShortcutsModal` | 1 / 1 | 安全 |
| `ResearchModal` / `SettingsModal` | 2 / 2 | 安全（side 布局固定高 720px） |
| **`EntityAlertRuleModal`** | **0** | **会被裁** |
| **`FeedAddModal`** | **0** | **会被裁** |
| **`FeedManageModal`** | **0** | **会被裁**（列表项多，最容易超） |
| **`GlobalModal`** | **0** | **会被裁**（confirm 文案长时） |
| **`InsightsModal`** | **0** | **会被裁**（图表 + 图例，xl 档 680×内容高） |
| **`OnboardingModal`** | **0** | **会被裁**（多步引导，文案最长） |

复核命令（0 = 没有内部滚动区）：

```bash
for f in EntityAlertRuleModal FeedAddModal FeedManageModal GlobalModal InsightsModal OnboardingModal; do
  printf "%-22s %s\n" "$f" "$(grep -c 'overflow-y: *auto\|overflow: *auto' src/components/$f.vue)"
done
```

**为什么它排在"风险"前面而不是"风险"里**：其余 R1–R9 都是"改法选错会难看/会不一致"，这一条是**改法对了也会坏**——字号只要放大，这 6 个弹窗就出现"底部按钮看不见、也滚不到"的内容丢失。而且它**在 1440×900 上不会暴露**，只在 1366×768 或系统缩放 150% 的机器上出现，review 时极易漏。

**前置动作（必须在动字号之前完成，属最小结构改动）**：给这 6 个弹窗的内容层包一层滚动容器。它们各自的 slot 根元素（已逐个核过类名）：

| 弹窗 | 要变成滚动容器的类 | 位置 | 备注 |
|---|---|---|---|
| `FeedManageModal` | `.manage-body` | `:29`、`:49`（两个 tab 各一个，都带 `manage-body`） | 头部 `.manage-head` 与底部 `.manage-foot` 是兄弟，可留在滚动区外 |
| `FeedAddModal` | `.add-body` | `:12` | 结构最简单，建议拿它当样板 |
| `GlobalModal` | `.modal-body` | `:37` | 三种形态（confirm / prompt / alert）共用 |
| `InsightsModal` | `.ins` | `:4` | 图表 + 图例 + 明细较长的那个 |
| `OnboardingModal` | `.onb` | `:7` | 步骤条 `.onb-steps` 与导航 `.onb-nav` 都在里面，需一并考虑是否固定 |
| `EntityAlertRuleModal` | `.arm` | `:9` | 外层还有 `v-if="entity"` |

统一写法（以 `FeedAddModal` 为例，写在宿主自己的 `<style scoped>` 里）：

```css
.add-body {
  flex: 1 1 auto;
  min-height: 0;          /* 关键：缺了它 flex 子项不会收缩，overflow 永远不生效 */
  overflow-y: auto;
  overscroll-behavior: contain;   /* 滚到底不把滚动传给背景 */
}
```

验收：**1366×768** 下逐个打开这 6 个弹窗，滚到底、底部按钮可见可点。这一条不过，字号改动的验收不成立。

### 5.2 `.btn-sm` 的 `12px` 是唯一不跟 token 的按钮字号

本文 §1.6 已确认 `.btn { font-size: var(--fs-sm) }`（`styles.css:611`）。需要补的是**它的兄弟档**：

```css
.btn-sm { height: 28px; padding: 0 var(--sp-3); font-size: 12px; }   /* styles.css:679-683 */
.btn-mini { height: 24px; padding: 0 var(--sp-2); font-size: var(--fs-xs); }  /* :684-688 */
```

`.btn-sm` 的 `12px` 是**写死的**，不消费任何 token；`.btn` 与 `.btn-mini` 都消费 token。于是按 R1 的建议只覆盖 `--fs-sm` 时：弹窗底部的 `.btn` 跟着变大、列表行内的 `.btn-sm` 原地不动 ⇒ **同一个弹窗里出现两档按钮字号**。

涉及 `.btn-sm` 的弹窗内文件：`SettingsAutomationTab`、`ResearchReportsTab`、`EntityDashboardModal`（另 3 个在 `EntityGraph` / `GraphView` / `TagPicker`，不在弹窗内）。

处理方式二选一，**不要不做**：
- 把 `12px` 换成 `calc(var(--fs-sm) - 2px)` 或新增 `--fs-btn-sm` 并同时覆盖——会让"按钮字号"重新变成单一来源；
- 或明确"`btn-sm` 保持 12px"，并写进 PR 描述，避免它在 review 里被反复当成 bug 提。

### 5.3 数字口径对账（两份统计不矛盾，别互相怀疑）

我按**"文件归属"**统计，本文按**"类归属"**统计，两者口径不同但可以互相校验：

| 口径 | 我的复算 | 本文 | 差异来源 |
|---|---|---|---|
| 弹窗内 `var(--fs-*)` | **125 处 / 21 文件**（12 个宿主 78 + 8 个 tab 38 + `MarketPanel` 9） | **128 处 / 23 文件** | **差额恰好是 `AppSelect` 的 3 处**（第 23 个文件）：它的触发器在面板内、下拉面板 Teleport 到 body，是否计入"弹窗侧"是口径选择。两份统计都不含 `TagPicker`（6 处，在"主界面"那一档，唯一调用点是 `ContentToolbar.vue:43`） |
| `styles.css` 全局规则 | 31 处（全部标为"共用"） | 8 处共用 + 23 处"只被弹窗消费" | 本文按"唯一消费者"再细分了一层，信息量更大 |
| 全仓总量 | **240 处**，两口径一致 | 240 处 | 一致 |

**实施时以本文的"按类归属"为准** —— 它回答的是"改这个类会打到谁"，那才是改动时要问的问题；我的"按文件归属"只用于核对"某个弹窗一共要吃多少处"。

另外三条独立复核结果，与本文一致，可作为交叉验证：`.tech-*` 42 条规则只被 `EntityDashboardModal`（经 `MarketPanel`）消费；`font-weight: 650` 全仓 3 处（`OnboardingModal:155` + `styles.css:1134` / `:1199`）；12 个宿主全部传 `:open`、无一处 `v-if`（`App.vue:38,40,44,46,52`、`FeedPanel.vue:220,234,237,239,245,247`、`ResearchEntitiesTab.vue:95`）。

---

## 附录：本文全部数字的核对方式

| 数字 | 命令 |
|---|---|
| 12 个弹窗文件 | `grep -rln "<BaseModal" src --include="*.vue"` |
| 全仓 `var(--fs-*)` 240 处 | `grep -ro "var(--fs-[a-z0-9]*)" src --include="*.vue" --include="*.css" \| wc -l` |
| 逐档 73 / 107 / 32 / 16 / 5 / 4 / 3 | `grep -rho "var(--fs-[a-z0-9]*)" src \| sort \| uniq -c` |
| 硬编码字号 37 处 | `grep -rn "font-size:" src/components \| grep -v "var(--fs" \| wc -l` |
| 弹窗动效分布 | §1.5 的 `for f in $(grep -rln "<BaseModal" …)` 循环 |
| 全局类使用面 | §1.6 的 python 片段（按 token 精确匹配） |
| `.tech-*` 26 类 / 单消费者 | `grep -o "\.tech-[a-z-]*" src/styles.css \| sort -u \| wc -l`；`grep -rln "tech-" src --include="*.vue"` |
| 结构伪类 0 处（组件内） | §R4 的 grep；**外加** `grep -n ":first-child\|:last-child\|:last-of-type" src/styles.css` |
| 关闭钮 9 个 | `for f in $(grep -rln "<BaseModal" src --include="*.vue"); do grep -n 'aria-label="关闭"' $f; done` |
| typecheck 基线 0 | `npx vue-tsc --noEmit; echo $?` |
| 未定义 token | §3.2(b) 的 python 片段 |
