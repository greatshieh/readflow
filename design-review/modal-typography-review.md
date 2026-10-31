# 弹窗体系 · 排版与视觉尺度审核

> 范围：14 个弹窗 / 浮层的**字号、字重、行高、字距、层级**。
> 不改布局结构、不改玻璃质感、不改无障碍能力、不改全局 token 的值。
> 下游：开发按 §4 的对照表逐文件改；§5 的 12 条验收全部可机械判定。
> 本文不含组件代码；CSS 片段只用来表达"改哪一行、改成什么值"。

---

## 0. 一句话结论

**弹窗不是"整体字号偏小 1px"，而是三件事同时缺位：层级差不够（13 个弹窗里 6 个的标题/正文只差 1px，1 个干脆同号）、行高没设（全应用 `line-height` 未定义，中文按字体 `normal` 挤在一起）、以及"角色"不存在（同一个 13px 在不同弹窗里是正文、标签、又或是区块标题）。**

所以处方不是"每个地方 +1px"，而是**给弹窗建立六个角色，角色绑到既有阶梯上**。改完之后：

| 量 | 现在 | 改后 |
|---|---|---|
| 标题 / 正文 层级比 | 1.00–1.23（13 个里 6 个 ≤1.077，1 个 =1.000） | **1.286**（18 / 14） |
| 正文行高 | 字体 `normal`（中文 ≈1.15–1.25） | **1.65** |
| 弹窗内裸写 px | 9 处 | **0** |
| 标题规格种数 | 6 种 + 3 个无标题 | **1 种** |
| 关闭钮 × 尺寸 | 22 / 16 / 15（三档） | **18 + 32×32 命中区** |

外加一个**结构性缺陷**（§2.8）：弹窗被 `Teleport` 到 `body`，而界面缩放 `zoom` 挂在 `.app` 上——**用户把界面字体调到「大」时，整个应用放大 1.1×，只有弹窗不放大**。弹窗里的字比周围小 9%，这大概率就是"弹窗看着不对劲"最直接的来源。

---

## 1. 现状体检表

字号真源：`src/styles.css:291-297`（`--fs-xs 11 / --fs-sm 13 / --fs-md 14 / --fs-base 14 / --fs-lg 16 / --fs-xl 18 / --fs-2xl 22`）。
`--fs-md` 与 `--fs-base` **同值 14**（`styles.css:293-294`）；两者当前用量 `--fs-md` 35 处、`--fs-base` 17 处。

**先校正两处事实**（我按行号核过，与派活说明略有出入，不影响结论）：

1. `SettingsModal` / `ResearchModal` 的**弹窗标题**是 `.side-head h3` = `--fs-base`(16)+600（`SettingsModal.vue:181`、`ResearchModal.vue:155`）；`--fs-md`(14)+600 的是右侧**标签栏标题** `.m-titlebar-title`（`:223` / `:205`）。也就是 side 布局弹窗里有两层标题。
2. `FeedManageModal` / `FeedAddModal` / `RulesModal` 的标题不是"没定义字号"，而是**字号的载体是容器**：`.manage-head` / `.add-head` 自己写着 `font-size: var(--fs-md); font-weight: 600`（`FeedManageModal.vue:256-264`、`FeedAddModal.vue:144-152`、`RulesModal.vue:420-428`），标题 `<span>` 是匿名元素、无类名、继承容器值。`ShortcutsModal` 同构但容器是 **500**（`ShortcutsModal.vue:62-71`）。

真正的"没标题"只有三个：`InsightsModal`、`EntityAlertRuleModal`、`TagPicker`。

| # | 弹窗 | 尺寸档 | 视觉标题（选择器:行） | 标题现状 | 正文用哪档 | 元信息用哪档 | 裸写值 / 异常 |
|---|---|---|---|---|---|---|---|
| 1 | `SettingsModal` | side 880 | `.side-head h3:181` | 16 / 600 / **-0.01em** | `.side-tab:194` 13 | — | 负字距（中文不该压） |
| 2 | `ResearchModal` | side 880 | `.side-head h3:155` | 16 / 600 | `.side-tab:170` 13 | — | — |
| 3 | `EntityDashboardModal` | 3xl 960 | `.dash-head-title:655` | **14 / 600** | `.dash-head-name:661` 14/600、`.dash-h:721` 13/600、`.dash-stat:749` 13、`.fin-v:774` 14/500、`.dash-empty:729` 13 | 11 × 5（`.badge:739`、`.fin-k:768`、`.fin-v em:779`、`.fin-p:784`、`.dash-reextract-hint:818`） | 标题与 `.dash-head-name` **同为 14/600** |
| 4 | `FeedManageModal` | sm 460 | 匿名 `<span>:8`（字号来自容器 `.manage-head:256`） | 14 / 600（继承） | `.manage-tab:291`→13、`.field-hint:353` 11/1.5 | `.manage-msg:278` 11、`.field label:323` 11 | `22px` ×（`:270`） |
| 5 | `InsightsModal` | xl 680 | **无** | — | `.ins-h:277` 14/700、`.ins-state:231` 13、`.ins-derived:336` 13 | `.ins-label:261` 11、`.ins-range-btn:287` 11、`.ins-axis:320` **11px 裸** | `1.5rem`=24px 大数字（`:255`）、`11px`（`:322`） |
| 6 | `ShortcutsModal` | md 520 | 匿名 `<span>:6`（字号来自容器 `.sc-head:62`） | 14 / **500**（继承） | `.sc-label:123` 13 | `.sc-gtitle:92` 11/500、`.sc-kbd:108` 11 | × 用 `var(--fs-lg)`(16)（`:75`） |
| 7 | `FeedAddModal` | sm 460 | 匿名 `<span>:7`（字号来自容器 `.add-head:144`） | 14 / 600（继承） | `.field-hint:192` 11/1.5 | `.add-msg:164` 11、`.field label:178` 11 | `22px` ×（`:158`） |
| 8 | `RulesModal` | sm 460 | 匿名 `<span>:7`（字号来自容器 `.manage-head:420`） | 14 / 600（继承） | `.hint:447` 13、`.rule-name:459` 13/500 | `.rule-desc:460` 11、`.field label:479` 11 | `22px` ×（`:434`）；`.form-title:476` 13/600 = 二级标题只比标题小 **1px** |
| 9 | `DigestModal` | lg 560 | `.m-head h3:234` | 16 / 600 | `.article-title:354` 14/500、`.article-summary:365` 13/1.5、`.empty-state p:269` 14 | `.article-source:350` 11、`.article-time:375` 11 | `22px` ×（`:244`） |
| 10 | `OnboardingModal` | md 520 | `.onb-title:152` | **18 / 650** | `.onb-desc:158` 14/1.6 | — | **650 非标准字重**（多数字体无此档，回落不可控） |
| 11 | `TagPicker`（浮层，非 BaseModal） | — | **无**（只有两个 tab，11px） | — | `.tp-empty:332` 11、`.tp-count:367` 11、`.tp-msg:433` 11 | — | `15px` × 2（`:318`、`:393`） |
| 12 | `EntityAlertRuleModal` | lg 560 | **无**（`.arm-info h4:257` 14/600 是"启用告警"这个区块的标题） | — | `.arm-info p:263` 13/1.5、`.arm-label:279` 13、`.arm-hint:334` 13 | — | — |
| 13 | `GlobalModal` | sm 460 | `.modal-title:216` | 14 / 600 | `.modal-message:244` 13/1.6 | `.input-hint:276` 11 | `22px` ×（`:228`） |
| 14 | `ArticleChat`（浮层，刻意非 BaseModal，见 `ArticleChat.vue:43`） | — | 无 | — | `ArticleChatPanel.vue:304` 14 | 11 × 2（`:343`、`:423`） | 本轮不动（见 §6） |

**字号分布**（校验过三处，与派活说明一致）：`EntityDashboardModal` = 11×5 + 13×4 + 14×4；`FeedManageModal` = 13×4 + 11×3 + 14×1；`RulesModal` = 13×4 + 11×3 + 14×1。**整个弹窗体系几乎全部活在 11–14px 区间，16px 以上的只有 3 个标题和 1 个 24px 的大数字。**

---

## 2. 诊断：为什么"呆板"

"呆板"是**信息没有主次**的观感，不是字体不好看。以下六条机制，每条都能落到可测的量。

### 2.1 层级差只有 1.00–1.08×（最主要的一条）

| 弹窗 | 标题 | 正文基准 | 比值 | 说明 |
|---|---|---|---|---|
| `EntityDashboardModal` | 14/600 | 14/600（`.dash-head-name`） | **1.000** | 完全同号同重，标题只是"排在上面的一行字" |
| `FeedManageModal` / `FeedAddModal` / `RulesModal` | 14/600 | 13/400 | **1.077** | 差 1px；靠 600 撑，见 §2.4 |
| `ShortcutsModal` | 14/500 | 13/400 | 1.077 | 连字重都只差一档 |
| `GlobalModal` | 14/600 | 13/400 | 1.077 | — |
| `SettingsModal` / `ResearchModal` / `DigestModal` | 16/600 | 13/400 | 1.231 | 勉强可辨 |
| `OnboardingModal` | 18/650 | 14/400 | 1.286 | 全体系唯一达标的，但用了非法字重 |
| `InsightsModal` / `EntityAlertRuleModal` / `TagPicker` | 无 | 13/400、11 | — | 没有标题可比 |

**13 个弹窗里 6 个的层级比 ≤1.077（其中 1 个正好 1.000），3 个 1.231，只有 1 个到 1.286，剩下 3 个没有标题。** 排版上，<1.15 的比值在 100% 缩放的显示器上读不出层级——读者看到的是"几行一样重的字排在一起"，这就是呆板。

### 2.2 行高从未定义

`src/styles.css` 里 `body`（`:430-441`）没有 `line-height`，`:root` / `html` 也没有（全文 `line-height` 只有 8 处，全在具体组件规则里）。于是弹窗里的中文全部按字体自带的 `normal` 排版 —— 中文字体的 `normal` 通常在 **1.15–1.25**，而 13px 正文的行盒只有 15–16px，汉字本身占满字身，行与行几乎贴着。

对照：`DigestModal` 的 `.article-summary` 明确写了 `line-height: 1.5`（`:368`），`GlobalModal` 的 `.modal-message` 写了 `1.6`（`:247`）—— **这两处是全弹窗体系里"看起来最像正文"的地方，恰好就是设了行高的两处**。这不是巧合。

### 2.3 角色不存在：同一个 13px 在五处在做五件事

以 `FeedManageModal` 为例，`--fs-sm`(13) 同时是：`.manage-tab`（页签）、`.field input`（输入框文字）、`.copy-btn`（按钮）、`.prompt-textarea`（长文本）、`.manage-title` 之外的列表主字段。而 `RulesModal` 的 13 又是 `.hint`（说明段落）、`.rule-name`（列表项）、`.form-title`（二级标题）。

**同一个字号承担了正文、控件、标题三种职责**，所以任何一处想调整，都会牵动另外两处 —— 这也是为什么现在"改一个字号会引发连锁"，最后谁都不敢动。

### 2.4 字重在中文里撑不起层级

可用的字重只有 `-apple-system` / `Segoe UI` / `Roboto` / 系统 sans 这几档。**中文字体普遍只提供 400 与 700 两个字重**；`font-weight: 500` / `600` 在缺档时由浏览器**合成加粗**或直接**回落 400**。这意味着：

- `EntityDashboardModal` 的 `.dash-head-title`(14/600) 与 `.dash-head-name`(14/600) —— 就算两者本意不同，渲染上完全一致；
- `RulesModal` 的标题 (14/600) 与 `.form-title` (13/600) —— 只差 1px，且这 1px 在同为 600 的情况下更难辨；
- `OnboardingModal` 的 `650`（`OnboardingModal.vue:155`）—— 650 在绝大多数中文字体里不存在，浏览器要么合成、要么取 700，**渲染结果不可控、跨平台不一致**。

结论：**不要用字重单独拉层级。字重只能做"补强"，层级的主结构必须由字号承担。** 这是本文所有数值的出发点。

### 2.5 11px 承载了本该读的文字

`--fs-xs`(11) 在弹窗里出现的位置包括：`.field-hint`（表单填法说明）、`.rule-desc`（规则描述）、`.article-source`（来源名）、`.sc-label` 之外的快捷键说明、`.ins-label`（图例）。其中 **`.field-hint` 与 `.rule-desc` 是解释性句子**——用户必须读懂才能正确操作。11px 的中文笔画密度极高，在 1× DPI 屏上基本只能"扫到轮廓"，于是用户的操作路径变成"试错"，观感上就是"这个弹窗不好用、很笨"。

### 2.6 负字距在压中文

`SettingsModal.vue:181` 的 `letter-spacing: -0.01em`、`InsightsModal.vue:258` 的 `-0.02em`：负字距是给拉丁字母排紧的排版手法；**汉字的字面本身就是满格**，压字距只会让字形互相挤、笔画粘连。这两处应当归零。

### 2.7 关闭钮三个尺寸 + 一个 24px 的"1.5rem"

| 位置 | 字形 | 命中区 |
|---|---|---|
| `FeedManageModal:270`、`FeedAddModal:158`、`RulesModal:434`、`DigestModal:244`、`GlobalModal:228` | **22px** | 无 `min-width/height`，`line-height: 1` + `padding: 0 4px` → 约 **30×22** |
| `ShortcutsModal:75` | 16px（`var(--fs-lg)`） | 约 24×16 |
| `TagPicker:318`（`.tp-close` 关闭）、`:393`（`.tp-del` 删除标签） | **15px** | 约 23×15 |
| `SettingsModal:228`、`ResearchModal:210` 的 `.btn-icon` | 28×28 盒 | 28×28 |

一个 × 有四种处理。22px 的 × 在一个 460px 的弹窗里是**视觉上最大的单个字符**，比标题还大——它天然抢注意力，这也是"廉价感"的来源之一。另外三处 15–16px 的命中区高度都不足 WCAG 2.5.8 要求的 24×24。

`InsightsModal.vue:255` 的 `1.5rem`：`html` 未设 `font-size`（`styles.css` 里没有 `html {}` 规则），所以它等于 **24px**，阶梯里没有这一档，也无法随界面缩放联动。

### 2.8 结构性缺陷：弹窗不随界面字号缩放

`src/utils/appearance.ts:154` 把用户选择的界面字号写成 `document.documentElement` 上的 `--ui-zoom`（`0.9 / 1 / 1.1 / 1.2`，见 `src/utils/appearance.ts:31-36`），而 `zoom: var(--ui-zoom, 1)` 只挂在 `.app` 上（`styles.css:448`）。

`BaseModal.vue:10` 用 `<Teleport to="body">` 把弹窗移到 `body` 下——**`.app` 之外**。于是：

| 界面字号 | 应用其余部分 | 弹窗 | 差距 |
|---|---|---|---|
| 小 0.9 | ×0.9 | ×1.0 | 弹窗大 11% |
| 中 1.0 | ×1.0 | ×1.0 | 一致 |
| 大 1.1 | ×1.1 | ×1.0 | **弹窗小 9%** |
| 特大 1.2 | ×1.2 | ×1.0 | **弹窗小 17%** |

旁证：`TagPicker` 与 `ArticleChat` 是**浮层**、渲染在 `.app` 内部，它们会跟着放大；只有真正的弹窗不跟。**同一个人调到「大」字，浮层变大了、弹窗没变** —— 这个不一致说明它是疏漏，不是设计选择。

---

## 3. 提案：弹窗六个角色

### 3.1 为什么选"角色 token"，而不是"在 `.base-modal` 里覆盖 `--fs-*`"

两种做法都能让弹窗不再写裸值，但代价完全不同：

| | (a) 新增弹窗角色 token（本文采用） | (b) 在 `.base-modal` 里重定义 `--fs-sm/--fs-md/...` |
|---|---|---|
| 改动面 | 14 个文件各自改类名（本来就要改标题） | 1 个文件 |
| 副作用 | 无：只有写了 `--fs-modal-*` 的元素变化 | **弹窗内所有子组件同时变化**，包括 `AppSelect` / `AppSwitch` / `kbd` / `input` / 以及从外部复用的 `TagPicker` |
| 可追溯 | 看到 `--fs-modal-body` 就知道"这是弹窗正文" | 看到 `--fs-sm` 不知道当前在哪个上下文，**同一个 token 在弹窗内外含义不同** |
| 与阶梯的关系 | 角色**别名**指向阶梯值，阶梯变了自动跟随 | 阶梯被就地改写，全局值失去唯一性 |
| 出错形态 | 漏改某个弹窗 → 它维持旧样，容易发现 | 漏改某个嵌套组件 → 它被无意放大，**很难发现** |

(b) 的本质问题是它让 `--fs-sm` 变成"取决于你在哪"的变量——这正是本项目反复吃亏的那类"第二个真源"。所以取 (a)。

### 3.2 六个角色（全部是既有阶梯的别名，**不新增任何数值**）

| 角色 | token | 值 | 来源 | 字重 | 行高 | 颜色 | 用在哪 |
|---|---|---|---|---|---|---|---|
| 弹窗标题 | `--fs-modal-title` | **18px** | `var(--fs-xl)` | 600 | **1.35** | `--text-primary` | 每个弹窗**恰好一个**：`.side-head h3`、`.manage-head > span`、`.m-head h3`、`.modal-title`、`.onb-title` |
| 区块标题 | `--fs-modal-section` | **16px** | `var(--fs-lg)` | 600 | **1.5** | `--text-primary` | `.dash-h`、`.ins-h`、`.sc-gtitle`、`.form-title`、`.tp-tabs` 之外的组头 |
| 正文 | `--fs-modal-body` | **14px** | `var(--fs-md)` | 400 | **1.65** | `--text-secondary` | 说明段落、消息、摘要、`.modal-message`、`.field-hint`、`.rule-desc` |
| 标签/列表主字段 | `--fs-modal-label` | **13px** | `var(--fs-sm)` | 500 | **1.4** | `--text-secondary` | `.field label`、`.manage-tab`、`.side-tab`、`.rule-name`、`.article-title`、`.sc-label`、`.arm-label` |
| 元信息 | `--fs-modal-meta` | **11px** | `var(--fs-xs)` | 400 | **1.5** | `--text-tertiary` | 时间、来源名、计数、徽标、坐标轴刻度 |
| 大数字 | `--fs-modal-num` | **22px** | `var(--fs-2xl)` | 700 | **1.1** | `--text-primary` + `font-variant-numeric: tabular-nums` | `.ins-num` 这类单值指标 |

**七条配套规则**（缺一条这套尺度就会塌）：

1. **每个弹窗只有一个 18px 标题。** 需要第二个"标题"时用区块标题 16px，不要再来一个 18。
2. **正文不降到 13。** 13px 只用于「标签、单行字段、页签」这类**不构成句子**的内容。
3. **11px 不承载解释性句子。** 只要那句话读完才能正确操作，它就属于正文（14/1.65）。
4. **层级主结构由字号承担，字重只做补强。** 禁止出现"同字号 + 不同字重"的两级（`EntityDashboardModal` 的 `.dash-head-title` / `.dash-head-name` 就是反例）。
5. **行高由 `.base-modal` 统一给**（1.65），个别角色在自己的规则里覆盖；**不要在每个选择器里重复写 1.65**，否则将来调一次要改几十处。
6. **中文不做负字距**，一律 `letter-spacing: normal`（即删掉两处负值，不新增声明）。
7. **关闭钮 ×：字形 18px，命中区 32×32。** × 是图形不是文字，不占角色位。

### 3.3 与现有阶梯的关系

- 六个角色**全部指向既有 11 / 13 / 14 / 16 / 18 / 22**，`--fs-*` 一个字都不改，全应用其余部分零影响。
- 只用别名而不直接写 `var(--fs-xl)`，是为了让"**这一处是标题**"这件事留在代码里；否则半年后又回到"看到 18px 不知道它凭什么 18"。
- `--fs-base` 与 `--fs-md` 同值 14，本文**不使用 `--fs-base`**。是否删除 `--fs-base` 需单独决策（当前 17 处在用，见 §6）。
- **不新增 12px / 15px / 24px 档位。** 顺带记录：`12px` 在 `RecommendView.vue` 出现 6 处、`15px` 在 `TagPicker`/`SettingsGeneralTab`/`ResearchTimelineTab` 共 4 处——这说明阶梯**确实缺一档 12**，但补档是全局决定（会让所有 `1.2rem` 类推断失效），不塞进本次弹窗改造（见 §6）。

### 3.4 改前 / 改后，同一屏的差异

以 `FeedManageModal`（sm 460，内容宽 428）为例：

| 量 | 现在 | 改后 | 变化 |
|---|---|---|---|
| 标题 | 14/600，行盒 ≈ 14×1.2 = 16.8 | 18/600，行盒 18×1.35 = **24.3** | 字号 +29%，层级比 1.077→**1.286** |
| 表单说明 `.field-hint` | 11/1.5，行盒 16.5 | 14/1.65，行盒 **23.1** | 字号 +27%，行盒 +40% |
| 表单标签 | 11/tertiary | 13/500/secondary | 从"弱提示"变成"字段名" |
| 一屏可读行数（86vh@900 = 774px） | ≈ 40 行（挤） | ≈ 26 行（松） | 见 §5 第 10 条上限 |

**信息没有变少，变的是"哪一行该被先看见"。**

---

## 4. 落地指引

### 4.1 第一步：在 `BaseModal.vue` 定义角色（唯一一处）

文件：`src/components/BaseModal.vue`，`.base-modal` 规则体内（**当前 `:265` 开始、`:287` 结束**，插在该规则的**第一行**，即 `position: relative;` 之前）。

```css
.base-modal {
  /* ── 弹窗排版尺度 ──
     六个角色全部是既有阶梯的别名：阶梯变，弹窗自动跟随；弹窗内不再出现裸 px。 */
  --fs-modal-title:   var(--fs-xl);    /* 18 */
  --fs-modal-section: var(--fs-lg);    /* 16 */
  --fs-modal-body:    var(--fs-md);    /* 14 */
  --fs-modal-label:   var(--fs-sm);    /* 13 */
  --fs-modal-meta:    var(--fs-xs);    /* 11 */
  --fs-modal-num:     var(--fs-2xl);   /* 22 */

  --lh-modal-title: 1.35;
  --lh-modal-section: 1.5;
  --lh-modal-body: 1.65;
  --lh-modal-label: 1.4;
  --lh-modal-meta: 1.5;
  --lh-modal-num: 1.1;

  /* 排版基线：任何未单独设字号的文字按"正文"渲染，
     而不是继承 body 的 14px + 字体自带 normal 行高 */
  font-size: var(--fs-modal-body);
  line-height: var(--lh-modal-body);
}
```

三点说明：

- **不设 `color`。** 保持继承 `body` 的 `--text-primary`，只有明确写了的元素才变次要色。少一个会被误伤的默认值。
- **`line-height` 只给这一层。** 各角色在自己的规则里写 `var(--lh-modal-*)`；不要在每个后代选择器里重复数字。
- 自定义属性**穿透** scoped 边界：`BaseModal` 的 scoped 样式不作用于插槽内容，但 `--fs-modal-*` 沿 DOM 继承，业务弹窗里可以直接用。

### 4.2 第二步：逐文件改类名（14 个，逐个给死值）

**A. 标题**（每个弹窗恰好一处，统一 18/600/1.35）：

| 文件 | 选择器:行 | 现在 | 改成 |
|---|---|---|---|
| `SettingsModal.vue` | `.side-head h3:181` | 16/600/-0.01em | `font-size: var(--fs-modal-title); font-weight: 600; line-height: var(--lh-modal-title);` **删掉 `letter-spacing`** |
| `ResearchModal.vue` | `.side-head h3:155` | 16/600 | 同上三行 |
| `DigestModal.vue` | `.m-head h3:234` | 16/600 | 同上三行 |
| `OnboardingModal.vue` | `.onb-title:152` | 18/**650** | `font-size: var(--fs-modal-title); font-weight: 600; line-height: var(--lh-modal-title);` **650 → 600** |
| `EntityDashboardModal.vue` | `.dash-head-title:655` | 14/600 | 同上三行；并把 `.dash-head-name:661` 改为 `var(--fs-modal-section)`(16)/600 —— 让"仪表盘"为标题、"实体名"为区块标题，两级分开 |
| `GlobalModal.vue` | `.modal-title:216` | 14/600 | 同上三行 |
| `FeedManageModal.vue` | `<span>` 在 `:8`，容器 `.manage-head:256` | 继承容器 14/600 | 给 `:8` 的 `<span>` 加 `class="m-title"`，新增 `.m-title { font-size: var(--fs-modal-title); font-weight: 600; line-height: var(--lh-modal-title); }`。**容器 `.manage-head`（`:256-264`）的 `font-size / font-weight / color` 三行删掉**（它现在只负责 flex 与边框） |
| `FeedAddModal.vue` | `<span>` 在 `:7`，容器 `.add-head:144` | 同上 | 同上（容器 `.add-head`（`:144-152`）同样删掉那三行） |
| `RulesModal.vue` | `<span>` 在 `:7`，容器 `.manage-head:420` | 同上 | 同上（容器 `.manage-head`（`:420-428`）同样删掉那三行）；`.form-title:476` 改为 `var(--fs-modal-section)`(16)/600 |
| `ShortcutsModal.vue` | `<span>` 在 `:6`，容器 `.sc-head:62` | 继承容器 14/**500** | 给 `:6` 的 `<span>` 加 `class="m-title"` + 同一条规则；容器 `.sc-head`（`:62-71`）删掉 `font-size: var(--fs-md); font-weight: 500;` 两行 |

**B. 三个没有视觉标题的，补标题**（`BaseModal` 的 `title` 属性只进 `aria-label`，屏幕上没有字）：

| 文件 | 现状 | 改成 |
|---|---|---|
| `InsightsModal.vue` | 头部只有 `.ins-chart-head`，无标题 | 在面板顶部加一行 `.m-title`，文字「阅读洞察」（与 `BaseModal` 的 `title="阅读洞察"` 一致），18/600 |
| `EntityAlertRuleModal.vue` | 头部第一行是 `.arm-info h4`「启用告警」(14/600) | 加一行 `.m-title`，文字「告警规则」；`.arm-info h4:257` 降为 `var(--fs-modal-section)`(16)/600 |
| `TagPicker.vue` | 只有两个 tab（11px） | **不加标题**（见 §6：它是浮层不是弹窗，加标题会破坏工具条的紧凑性），仅把 `.tp-tab:303` 提到 `var(--fs-modal-label)`(13) |

**C. 正文 / 标签 / 元信息**（按角色批量替换，逐处给值）：

| 文件 | 选择器:行 | 现在是 | 改成 |
|---|---|---|---|
| `FeedManageModal.vue` | `.field label:323` | 11/tertiary | `--fs-modal-label` / `--lh-modal-label` / 500 / `--text-secondary` |
| `FeedManageModal.vue` | `.field-hint:353` | 11/1.5 | `--fs-modal-body`(14) / `--lh-modal-body` / `--text-secondary`；删掉 `line-height: 1.5` |
| `FeedManageModal.vue` | `.manage-msg:278` | 11 | `--fs-modal-body`(14)（这是给用户看的错误信息，必须能读） |
| `FeedManageModal.vue` | `.manage-tab:291` | 13 | `--fs-modal-label`(13) / 500 |
| `FeedAddModal.vue` | `.field label:178` | 11/tertiary | 同 A 表对应的 label 行 |
| `FeedAddModal.vue` | `.field-hint:192` | 11/1.5 | `--fs-modal-body`(14)，删 `line-height` |
| `FeedAddModal.vue` | `.add-msg:164` | 11 | `--fs-modal-body`(14) |
| `RulesModal.vue` | `.hint:447` | 13 | `--fs-modal-body`(14) |
| `RulesModal.vue` | `.rule-name:459` | 13/500 | `--fs-modal-label`(13) / 500 保持 |
| `RulesModal.vue` | `.rule-desc:460` | 11 | `--fs-modal-body`(14)——它是规则描述，用户靠它判断该不该开 |
| `RulesModal.vue` | `.field label:479` | 11/tertiary | `--fs-modal-label` / `--text-secondary` |
| `DigestModal.vue` | `.article-title:354` | 14/500/1.4 | `--fs-modal-label`(13)/500 或保持 14/500 —— **取 13/500**，与正文 13 拉开 |
| `DigestModal.vue` | `.article-summary:365` | 13/1.5 | `--fs-modal-body`(14) / `--lh-modal-body` |
| `DigestModal.vue` | `.article-source:350`、`.article-time:375` | 11 | `--fs-modal-meta`(11)，保持 |
| `InsightsModal.vue` | `.ins-num:255` | `1.5rem`/700/-0.02em | `--fs-modal-num`(22) / 700 / `--lh-modal-num`；**删 `letter-spacing`** |
| `InsightsModal.vue` | `.ins-h:277` | 14/700 | `--fs-modal-section`(16)/600 |
| `InsightsModal.vue` | `.ins-axis:320` | **11px 裸** | `--fs-modal-meta`(11)，保留 `tabular-nums` |
| `InsightsModal.vue` | `.ins-state:231`、`.ins-derived:336` | 13 | `--fs-modal-body`(14) |
| `OnboardingModal.vue` | `.onb-desc:158` | 14/1.6 | `--fs-modal-body`(14) / `--lh-modal-body` |
| `EntityDashboardModal.vue` | `.dash-h:721` | 13/600 | `--fs-modal-section`(16)/600 |
| `EntityDashboardModal.vue` | `.dash-stat:749`、`.dash-empty:729` | 13 | `--fs-modal-body`(14) |
| `EntityDashboardModal.vue` | `.fin-v:774` | 14/500 | `--fs-modal-body`(14)/400（它是"键值对的值"，不是强调） |
| `EntityDashboardModal.vue` | `.badge:739`、`.fin-k:768`、`.fin-p:784`、`.dash-reextract-hint:818` | 11 | `--fs-modal-meta`(11) |
| `EntityAlertRuleModal.vue` | `.arm-info p:263`、`.arm-label:279`、`.arm-hint:334`、`.arm-error:323` | 13 | `--fs-modal-body`(14)（`.arm-label` 改 `--fs-modal-label` 13/500） |
| `GlobalModal.vue` | `.modal-message:244` | 13/1.6 | `--fs-modal-body`(14) / `--lh-modal-body` |
| `ShortcutsModal.vue` | `.sc-label:123` | 13 | `--fs-modal-label`(13) 保持 |
| `ShortcutsModal.vue` | `.sc-gtitle:92` | 11/500 | `--fs-modal-section`(16)/600（组名是区块标题，不是元信息） |
| `ShortcutsModal.vue` | `.sc-kbd:108`、`.sc-foot:128` | 11 | `--fs-modal-meta`(11) |
| `TagPicker.vue` | `.tp-empty:332`、`.tp-count:367`、`.tp-msg:433` | 11 | `--fs-modal-meta`(11) / `.tp-empty` 改 `--fs-modal-body`(14) |

### 4.3 第三步：关闭钮统一（7 处 × 字形 + 2 处 `.btn-icon`）

统一成一条规则（各文件替换自己的 `.manage-close` / `.add-close` / `.m-close` / `.modal-close-btn` / `.sc-close` / `.tp-close`）：

```css
.xxx-close {
  font-size: var(--fs-xl);        /* 18 —— 字形，不是文字角色 */
  line-height: 1;
  min-width: 32px;
  min-height: 32px;
  display: grid;
  place-items: center;
  margin: -8px -8px -8px 0;       /* 抵掉 32 盒多占的空间，头部高度不变 */
  color: var(--text-tertiary);
}
```

- 5 处 `22px`（`FeedManageModal:270`、`FeedAddModal:158`、`RulesModal:434`、`DigestModal:244`、`GlobalModal:228`）、`ShortcutsModal:75` 的 `var(--fs-lg)`、`TagPicker:318/393` 的 `15px` 全部换成上面的形状。
- `GlobalModal.vue:222-223` 那段注释（「与其余弹窗的关闭按钮…对齐」）**同时失效**，改成指向本文 §3.2 第 7 条。
- `SettingsModal` / `ResearchModal` 的 side 布局关闭钮是 `.btn-icon`（28×28，`SettingsModal.vue:228`、`ResearchModal.vue:210`，两处各定义一遍）。**本轮只把两处重复定义合并为同值、并按上表补到 32×32 命中区**，不改结构。

### 4.4 第四步（独立提交）：让弹窗跟随界面字号

这是 §2.8 那个结构性缺陷的修法。它改的是共享外壳，**建议单独立一个提交**，便于回滚。

```css
/* BaseModal.vue · .overlay（当前 :238-253） */
.overlay {
  /* zoom 下 inset: 0 会按缩放后的单位解析，遮罩会超出视口、把面板推出中心。
     所以用 calc 反算，让遮罩在 zoom 之后恰好等于视口。 */
  position: fixed;
  top: 0;
  left: 0;
  width: calc(100vw / var(--ui-zoom, 1));
  height: calc(100vh / var(--ui-zoom, 1));
  zoom: var(--ui-zoom, 1);
  /* 其余（z-index / display / 背景 / 过渡 / global 变体）不动 */
}

/* .base-modal（当前 :265-287）追加两行，让面板不越过视口 */
  max-width: calc(92vw / var(--ui-zoom, 1));
  max-height: calc(86vh / var(--ui-zoom, 1));
```

- `size-*` 的 460/520/560/680/820/960 是固定 px，会被 `zoom` 正常放大，**不需要改**。
- 验收：见 §5 第 11 条（四档缩放量 computed font-size）。

---

## 5. 验收标准（12 条，全部可机械判定）

| # | 项 | 通过标准 |
|---|---|---|
| 1 | 无裸写字号 | `grep -rn "font-size: *[0-9]" src/components/*Modal*.vue src/components/BaseModal.vue src/components/TagPicker.vue` = **0 命中**（现在是 9 处） |
| 2 | 无非标准字重 | `grep -rn "font-weight: *[0-9]" src/components/*Modal*.vue` 里只允许 400 / 500 / 600 / 700；**650 必须为 0** |
| 3 | 无 `rem`/`em` 字号 | `grep -rn "font-size: *[0-9.]*rem\|font-size: *[0-9.]*em" src/components/*Modal*.vue` = 0（现在是 `InsightsModal:255` 的 `1.5rem`） |
| 4 | 标题唯一且达 18 | 每个业务弹窗文件内 `--fs-modal-title` **恰好出现 1 次**；且不存在 `--fs-modal-title` 与 `--fs-modal-body` 指向同值的情况 |
| 5 | **最小层级差** | 逐弹窗比对：标题 18 vs 正文 14 → **比值 1.286 ≥ 1.25，且绝对差 4px ≥ 4px**。任何两处"同字号不同字重"当层级用的写法必须为 0（`EntityDashboardModal` 的 `.dash-head-title` / `.dash-head-name` 是重点复查点） |
| 6 | **正文最小字号** | 弹窗内**整句文字**（含 `。，` 或长度 >16 字）不得使用 11px 或 13px；`--fs-modal-meta` 只允许出现在时间 / 来源 / 计数 / 徽标 / 刻度五类元素上 |
| 7 | 行高到位 | `.base-modal` 内存在 `line-height: var(--lh-modal-body)`；`--lh-modal-body` 为 1.65。抽查 3 个正文元素 computed `line-height` = 字号 ×1.65（14 → 23.1px，允许 ±0.5） |
| 8 | 无负字距 | `grep -rn "letter-spacing: -" src/components/*Modal*.vue` = **0**（现在 2 处） |
| 9 | 关闭钮一致 | **7 处 × 字形**的 `font-size` 均为 `var(--fs-xl)`(18)、`min-width` 与 `min-height` 均为 32px；**2 处 `.btn-icon`** 命中区也是 32×32。`grep -rn "font-size: 22px\|font-size: 15px" src/components/*Modal*.vue src/components/TagPicker.vue` = **0**（现在 7 处） |
| 10 | **单屏信息密度上限** | 在 900px 高窗口（面板 86vh = 774px）下，`size="sm"`(460) 与 `size="lg"`(560) 弹窗**首屏可见正文行数 ≤ 26 行**（按行盒 23.1px 计）；且首屏必须完整包含「标题 + 关闭钮 + 主操作按钮」。超出即说明有内容该进折叠区或滚动区，而不是把字号压小 |
| 11 | 缩放跟随（阶段二） | devtools 量标题 computed `font-size`：界面字号 小/中/大/特大 → **16.2 / 18 / 19.8 / 21.6 px**（误差 ≤0.5）。同时确认遮罩铺满视口、面板居中（不偏移） |
| 12 | 无回归 | 改完后 `npm run build` 通过；暗色主题下逐个弹窗截图，正文/元信息仍可读（`--text-secondary` / `--text-tertiary` 在暗色下由 `styles.css:400-404` 提供，本轮不改） |

---

## 6. 这次不做什么（都有理由，请不要顺手改）

| 不改 | 为什么 |
|---|---|
| **全局 `--fs-*` 的值** | `--fs-sm` 从 13 改 14 会动到全应用 100+ 处，且不是用户要的（他要弹窗不呆板，不是全应用重排）。角色 token 就是为了绕开这个风险 |
| **删掉冗余的 `--fs-base`** | 它和 `--fs-md` 同值，确实是冗余；但当前 17 处在用，删除是纯机械但跨文件的改动，应单独排期。本文只保证**弹窗不再新增** `--fs-base` 用法 |
| 新增 12px 档位 | `12px` 在 `RecommendView` 出现 6 处、`15px` 在 3 个文件出现 4 处，说明阶梯确实缺档；但补档会影响所有 `rem` 推断，属全局决策，**不在本次范围**。本次用「14 正文 + 11 元信息」两档把 12 的需求吸收掉 |
| `BaseModal` 的 `size` 档位宽度 | 460/520/560/680/820/960 与 `layout="side"` 的 880 是布局决定，字号变化不影响它们成立 |
| 焦点陷阱 / `aria-*` / Esc 栈 / `mousedown` 关遮罩 | 无障碍能力已验收，本文一个字都不碰 |
| 玻璃质感、阴影、圆角、遮罩模糊 | 观感层已经调过（`--glass-strong` / `blur(28px)` / `--shadow-pop`），与本轮"字"的问题正交 |
| `TagPicker` 套用弹窗角色表 | 它是**浮层不是弹窗**（`TagPicker.vue` 自己不用 `BaseModal`），宽 320px、紧贴工具条、内容是高密度标签列表。用 14/1.65 会把列表拉松、破坏紧凑性。本轮只做三件事：× 归 18、两处 15px 归位、`.tp-tab` 11→13 |
| `ArticleChat` / `ArticleChatPanel` | `ArticleChat.vue:43` 明确写了"非模态浮层，刻意不用 BaseModal"。它已经在用 14/13/11 且行高合理，本轮不动 |
| 把 × 换成 SVG 图标 | 视觉上确实更整齐，但会引出"图标尺寸/描边/对齐"一整套新规范，属另一次改动 |
| 给弹窗加图标或强调色条 | 会引入新颜色（违反紫罗兰单色约束），且图标库选择是独立决策 |
| `RecommendView.vue` 的 12/15/18px | 同一类问题（跳过阶梯），但不在弹窗范围。**登记备查**：`RecommendView.vue:223/229/238/253/286/290/298/324/339/350/367` 共 11 处裸写字号，建议另开一次 |

---

## 7. 顺带发现的三处"文档不实"（不影响本次改动，但会让开发走错）

1. `GlobalModal.vue:222-223` 注释说「22px：与其余弹窗的关闭按钮（RulesModal / **ResearchModal** / DigestModal / FeedManageModal）对齐」——`ResearchModal` 里没有 22px 的关闭钮，它用的是 `28px` 的 `.btn-icon`（`ResearchModal.vue:210`）。
2. `BaseModal.vue:55` 注释说 side 布局「自带面板右上角关闭钮（…关闭钮由外壳统一提供）」，但模板里只有 `<slot />`（`BaseModal.vue:21`），**外壳并没有渲染关闭钮**；实际的关闭钮在宿主的 `.m-titlebar` 里（`SettingsModal.vue:28`、`ResearchModal.vue:29`）。
3. `EntityDashboardModal.vue:14` 注释说「关闭钮复用全局 `.btn-icon`」——`styles.css` 里没有 `.btn-icon`，它是在 `SettingsModal.vue:228` 与 `ResearchModal.vue:210` **各定义了一遍**。

---

## 附：本文所有数字的复核命令

```bash
# 字号阶梯真源
sed -n '289,298p' src/styles.css

# 行高是否存在全局默认（答案：0 处）
grep -n "line-height" src/styles.css

# 每个弹窗的排版声明（选择器 + 行号 + 声明）
for f in src/components/*Modal*.vue; do echo "## $f"; grep -n "font-size\|font-weight\|letter-spacing\|line-height" $f; done

# 裸写字号（当前 9 处，均在弹窗/浮层范围）
grep -rn "font-size: *[0-9]" src/ | grep -v "var(--fs"

# 非标准字重（当前 1 处：OnboardingModal:155 的 650）
grep -rn "font-weight: *[0-9]" src/components/*.vue | grep -vE "font-weight: *(400|500|600|700)\b"

# 界面缩放档位与写入点
sed -n '31,36p' src/utils/appearance.ts
sed -n '153,155p' src/utils/appearance.ts
grep -n "zoom: var(--ui-zoom" src/styles.css

# 两个同值 token 的用量
grep -rn -- "--fs-md"   src/ | wc -l   # 35
grep -rn -- "--fs-base" src/ | wc -l   # 17
```
