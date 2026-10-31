# 弹窗体系 · 交互与动效审核

> 范围：14 个弹窗 / 弹层外壳与内部反馈的**触发、过渡、时长、曲线**。
> 姊妹篇：`modal-typography-review.md`（字号 / 字重 / 行高 / 角色 token）——**本文不重复那部分**，也不改任何排版数值。
> 下游：开发按 §3.3 的对照表逐文件改；§3.1 的四个时长 + 两条曲线是唯一真源。
> 本文不含组件代码；CSS / JS 片段只用来表达"加在哪一行、写成什么"，可以直接照抄。
> **不改** `BaseModal` 的无障碍判据（焦点陷阱、焦点归还、Esc 让路、`mousedown` 关遮罩）。

---

## 0. 一句话结论

**"不流畅"不是"动画太少"，是三件事叠在一起：进场有形体、退场没有形体（关弹窗只有遮罩在淡）；弹窗内的反馈时长有两套并存（0.15s / 0.22s），且都不是为"这个动效该多快"定的；以及最有感的那一层——键盘焦点在弹窗里根本看不见。**

用户说的"不是整体呆板，而是弹窗"，与代码事实吻合：**全局 `button` 基线本来已经给了 0.22s 六属性过渡和 `:active` 回弹（`src/styles.css:567-573`），但这套手感在弹窗里正好被 8 处 `transition: all 0.15s` 整条覆盖。** 也就是说，弹窗内的手感不是"在基线之上做加法"，而是"把基线换掉了一次"——而且换成了**更快的 0.15s**。于是同一个屏幕里，外壳按 220ms 进来、里面的页签按 150ms 变色：**慢的那层是"出现"，快的那层是"操作"，节奏正好反了。**

改完之后：

| 量 | 现在 | 改后 |
|---|---|---|
| 退出动效 | **无**（面板 0ms 消失，只剩遮罩 220ms 淡出） | **120ms 独立退出**（位移 + 淡出） |
| 时长档数 | 弹窗内 2 个值（0.15 / 0.22）、全仓 6 个值（0.15 / 0.2 / 0.22 / 0.25 / 0.3 / 0.32），**没有一个是按用途定的** | **4 档，按用途定义**（90 / 160 / 200 / 120） |
| `transition: all` | 8 处（弹窗内） | **0 处** |
| 弹窗内可 Tab 元素的焦点环 | **0 处**（全局只覆盖 input/textarea/select） | **全弹窗覆盖** |
| `prefers-reduced-motion` | 弹窗 0 处（全应用 6 处，一处都没落在弹窗上） | **1 处覆盖全部** |

---

## 1. 交互体检表

### 1.0 先校准三个前提（派活说明里有出入，不影响结论）

**① 外壳并非"只有进入动画"，而是"进入有 animation、退出连 animation 都没有"。**

`BaseModal.vue:290-292` 把动画挂在 `.overlay.show .base-modal` 上，`@keyframes base-modal-in` 是 `opacity 0→1` + `translateY(8px→0)`。关闭时 `.show` 被移除，**animation 声明随之消失，元素立即回到 base 态（`opacity:1` / `translateY(0)`）**——不是"反向播放"，是"撤销播放"。于是退出期间面板没有任何独立运动，只是被父级 `.overlay` 的 `opacity` 淡出带着一起变淡（`BaseModal.vue:252`）。

**结论：退出的问题不是"少了 200ms"，是"少了形体"。** 进场看得见"它从下方浮上来"，退场只看得见"它整体变少"。

**② 四个"transition 计数为 0"的弹窗，不等于"悬停全是硬跳"。**

`src/styles.css:567-572` 有一条**全局** `button` 规则：

```css
button {
  transition: background var(--dur) var(--ease), color var(--dur) var(--ease),
    border-color var(--dur) var(--ease), box-shadow var(--dur) var(--ease),
    transform var(--dur) var(--ease), opacity var(--dur) var(--ease);
}
button:not(:disabled):active { transform: scale(0.96); }
```

它按 0.22s 覆盖了 background / color / border-color / box-shadow / transform / opacity。所以**只要是 `<button>` 且属性在 hover 时变化，就已经有 0.22s 过渡**，不硬。逐条核过之后，真正"硬跳"的只有 **2 处**，而且都不是按钮：

| 位置 | 元素 | hover 改什么 | 为什么硬 |
|---|---|---|---|
| `ShortcutsModal.vue:106` `.sc-row:hover` | `<div>` | `background` | 非 button，全局基线不生效，本文件 0 条 transition |
| `TagPicker.vue:388` `.tp-name-input:hover` | `<input>` | `border-color` | 非 button；`input` 没有全局过渡规则 |

**③ 真正"零反馈"的是 `EntityDashboardModal`，而且它是另一种病。**

它全文 0 条 `:hover`、0 条 `transition`。里面的可点元素是 `.tech-link`（`EntityDashboardModal.vue:137`、`:190`）和 `.tech-collapse-fab`（`:211`）——都是 `<button>`，会走全局基线，**但没有任何 hover 态可过渡**。所以它的问题不是"过渡太快"，是**没有设计悬停态**：鼠标移上去什么都不变。修法不是加时长，是加 `:hover` 声明（§3.3 第 4 行）。

### 1.1 逐弹窗（14 个）

> "外壳"= `BaseModal` 提供的进入/退出/遮罩，13 个弹窗完全相同；下表只记录**内部**差异。`TagPicker` 不用 `BaseModal`（自绘浮层），单列。

| # | 弹窗 | 外壳 | 悬停反馈 | 切换 / 展开 | 加载 / 空态 | 焦点可见 | 硬跳 / 缺口 |
|---|---|---|---|---|---|---|---|
| 0 | `BaseModal`（外壳，13 个共用） | 进入 220ms 上浮 8px + 淡入；**退出无动效** | — | — | — | 面板 `outline: none`（`:286`），无焦点环 | **缺退出**；遮罩 `visibility` 也参与过渡（`:252`），这是后面 §3.2 要小心的点 |
| 1 | `SettingsModal` | 同外壳 | `.side-tab:203` / `.btn-icon:241` 均 `transition: all 0.15s`（`:201`、`:239`） | 切 tab 是 `v-if` 直接换 DOM（`:41`），**瞬替**；外侧固定高度所以不跳高 | — | 无 | `all` 会连带过渡布局属性 |
| 2 | `ResearchModal` | 同外壳 | 同上（`:179`、`:221`） | 同 `SettingsModal` | — | 无 | 同上 |
| 3 | `EntityDashboardModal` | 同外壳 | **无任何 hover 态** | 折叠成 FAB / 召回：`v-if` 瞬现瞬消（`:211`） | 重抽按钮有 disabled 态，无过渡；**无 loading 动效** | 无 | **缺悬停**（不是"硬"） |
| 4 | `ShortcutsModal` | 同外壳 | `.sc-close`（button）走全局 0.22s；`.sc-row`（div）**硬跳** | — | — | 无 | 1 处硬跳 |
| 5 | `RulesModal` | 同外壳 | `.manage-close`（button）走全局 | 规则增删是列表 `v-if`，瞬变 | — | 无 | — |
| 6 | `TagPicker`（浮层，非 BaseModal） | **零动效**：`v-if` 挂载，瞬现瞬消 | `.tp-tab`/`.tp-close`/`.tp-row`/`.tp-del`（button）走全局；`.tp-name-input` **硬跳** | 两个页签 `v-if` 瞬替 | 空态文案瞬现 | `.tp-name-input:focus` 只换 `border-color`（`:388`），**无过渡**；按钮无焦点环 | 1 处硬跳 + 整体瞬现瞬消 |
| 7 | `FeedManageModal` | 同外壳 | `.manage-tab` `color/border-color 0.15s`（`:302`）；`.copy-btn` 走全局 | 页签 `.active` 只改 `color`/`border-bottom-color`，两者都在过渡清单里 → **有过渡**（`:305-308`） | — | 无 | 过渡清单漏了 `background`，一旦将来给 tab 加底色就会硬跳 |
| 8 | `InsightsModal` | 同外壳 | `.ins-range-btn` `all 0.15s`（`:295`） | 换时间范围会触发 `load()`，**重载期间无骨架、无过渡** | 只有 `.ins-state` 文案 | 无 | 数据重载的"空窗"没有任何过渡 |
| 9 | `FeedAddModal` | 同外壳 | `.add-close` 走全局 | — | — | 无 | — |
| 10 | `DigestModal` | 同外壳 | `.digest-article`（div）`background 0.15s`（`:321`）——**div 有过渡，是这轮唯一"非按钮也做了过渡"的地方** | 整行点击即跳转 | — | 无 | `.digest-article` 是 `<div @click>`（`:38`），**不可聚焦** |
| 11 | `OnboardingModal` | 同外壳 | 只有 `transition: background var(--dur)`（`:142`） | 步骤切换 | — | 无 | 逐页切换无动效 |
| 12 | `EntityAlertRuleModal` | 同外壳 | `.arm-chip` `all 0.15s`（`:296`）、`.arm-cancel`/`.arm-save`（`:348`、`:362`） | chip 选中走 `all`，**有过渡** | — | 无 | `all` 同样波及布局属性 |
| 13 | `GlobalModal` | 同外壳（`z-index: 9999`，`BaseModal.vue:260-262`） | `.modal-close-btn` 走全局 | — | — | 无 | 叠在弹窗之上时**两层遮罩各自模糊**（见 §4.3） |

**一行汇总：13 个 BaseModal 弹窗共享同一套"进有形、出无形"的壳；内部有 10 个弹窗完全没有为"切换/展开/加载"设计过过渡，只有 4 个碰巧写了；焦点环全体系 0 处。**

---

## 2. 诊断：把"不流畅"拆成可归因的八类

### 2.1 退场没有形体（最主要）

进场是"从下方浮上来 8px + 淡入"，退场是"整块变淡"。人对**运动**的记忆比对**透明度**强得多：有位移的消失看起来是"它走了"，没位移的消失看起来是"它被删了"。这是"关弹窗感觉硬"最直接的来源。

而且退场用的曲线是 `--ease: cubic-bezier(0.4, 0, 0.2, 1)`——**两端减速**。两端减速的曲线用在"消失在最后"上，末段速度最慢，主观上就是"关不干净、拖一下才没"。

### 2.2 时长有两个值，且没有一个是按用途定的

| 时长 | 出现处 | 实际承担的用途 | 问题 |
|---|---|---|---|
| `0.15s` | **弹窗内 17 处**（裸写，不含 token）：8 处 `transition: all` + 9 处显式属性 | 悬停 / 选中 / 输入框聚焦 | 比外壳的 220ms **快 32%**，于是"打开慢、里面快"，两层节奏打架 |
| `0.22s` | `--dur`：外壳遮罩与面板 + 全局 `button` 基线 | 悬停 / 面板进入 | 悬停用它太慢（见 §3.1 的 T1 论证）；进入用它刚好 |
| `0.2s` / `0.25s` / `0.3s` / `0.32s` | 弹窗外（`ArticleSortBar:107`、`styles.css:786`、`:771`、`--dur-slide`） | 各处自定义 | 与弹窗无关，但正好说明时长是"每次现挑"的 |

**核心问题不是"数值不统一"，是没有任何一条规则回答"这个动效该多快"。** 同一个"悬停变色"，在外壳里 220ms、在 `SettingsModal` 里 150ms、在 `EntityDashboardModal` 里没有。用户感知到的是"这个弹窗反应迟钝、那个弹窗一碰就变"。

而 17 处 `0.15s` 里 **8 处是 `transition: all`**，这把问题从"数值不对"升级成"过渡范围也不对"（§2.3）。

### 2.3 `transition: all` 8 处，把"过渡"变成了"猜"

`SettingsModal:201/239`、`ResearchModal:179/221`、`InsightsModal:295`、`DigestModal:397`、`EntityAlertRuleModal:296/348`。

`all` 的问题有两个，都不是理论问题：

1. **它会过渡任何属性，包括布局属性。** 这些元素里有 `padding` / `border-bottom-width` 会在状态间变化的（如 `.manage-tab` 的 `border-bottom: 2px`），一旦某天有人改了 padding，`all` 会把布局过渡一起演出来——**而布局过渡 = 每帧重排**，在 28px 模糊的玻璃面板上直接掉帧。
2. **它是覆盖全局基线的元凶。** `button` 的 0.22s 基线被 `.side-tab { transition: all 0.15s }` 整条替换，于是同一屏里"关闭钮 220ms、页签 150ms"。

### 2.4 弹窗里没有任何焦点环（最有感，也最容易被当成"不流畅"）

`src/styles.css:690-695` 只给 `input / textarea / select` 做了 `:focus-visible`（`outline: none` + `border-color` + `box-shadow: 0 0 0 3px`）。**按钮没有全局焦点环。**

全仓有 4 处手写了 `:focus-visible`，写法完全一致（`outline: 2px solid var(--primary); outline-offset: 2px`）：

- `AppSwitch.vue:112-114`
- `AppTooltip.vue:162-164`
- `FeedListItem.vue:255-257`（`:focus-visible`，`:offset -2px`）
- `ToastHost.vue:102`、`:118`

**这 4 处一处都不在弹窗里。** 而弹窗是"焦点陷阱"最严密的地方——Tab 在这 14 个弹窗里循环，用户看不见自己停在哪。键盘用户的主观体验不是"不流畅"，是"这个东西坏了"。

**还要纠正一个容易搞反的事实**：弹窗里确实有 8 处 `outline: none`（`EntityAlertRuleModal:314`、`FeedAddModal:187`、`FeedManageModal:332/371`、`GlobalModal:266`、`RulesModal:488`、`TagPicker:388/420`），但**全部挂在输入框上**，而且每一处都配了 `border-color` 的 `:focus` 态——那是合理做法，**不要动**。

所以准确的说法不是"焦点环被去掉了"，而是：**按钮的焦点环从来没有被画过。** 这两件事的修法不一样：前者要"删一条覆盖"，后者要"加一条规则"。

> 这条与无障碍规范归口交叉，但**改动本身是纯视觉的**：加一条 `outline` 规则，不触碰 `BaseModal` 的焦点陷阱/归还逻辑（`BaseModal.vue:149-196`、`:203-227` 一行不动）。

### 2.5 滚动锁带来 4px 位移（不是动效问题，但同属"一顿"）

`src/utils/dialogStack.ts:34` 打开时 `document.body.style.overflow = 'hidden'`，`:47` 关闭时复位。全应用滚动条宽 4px（`src/styles.css:546-549`，`:root` / `::-webkit-scrollbar`）。

于是**每次开关弹窗，背景内容横向跳动 4px**。4px 很小，但它发生在"画面最亮的那一刻"（弹窗刚出现的瞬间），人的眼睛正好在看背景——这是那种"说不上哪里不对"的来源。

修法一行：`html { scrollbar-gutter: stable; }`（滚动条槽位常驻，不随 `overflow` 变化）。

### 2.6 状态切换是"换 DOM"，不是"变状态"

三处典型：`SettingsModal.vue:41` 的 `v-if="tab === 'general'"`、`TagPicker` 的两个页签、`EntityDashboardModal` 的折叠/召回（`:208`）。

`v-if` 的行为是**销毁再创建**：内容瞬间替换、内部滚动位置归零、进入动画不会播放。对 side 布局的 tab 切换，这是**可以接受的**（见 §5 第 1 条：tab 内容不该加淡入）；但对"折叠成 FAB / 从 FAB 召回"这种**同一个对象在两种形态间切换**的动作，瞬变会让人怀疑"我是不是把它关掉了"。

### 2.7 一处容易被误判的：玻璃面板上的动画成本

`.base-modal` 有 `backdrop-filter: blur(28px) saturate(1.7)`（`BaseModal.vue:275-276`），遮罩有 `blur(10px)`（`:247-248`）。**面板一旦移动，它背后的取样区域就变了，模糊必须逐帧重算。**

现在的进场 `translateY(8px)` 已经是这个成本的既有实例——**它可能就是"打开弹窗卡一下"的物理原因**，而不是任何时长参数的问题。所以 §3.2 没有把它加到 10px，反而收到 **6px 进出共用**（一份位移、两个方向各付一次的一半），理由在 §4.1。

### 2.8 关闭后 220ms 内点击被吞（一个注释写反了的 bug）

**这是本轮审核唯一找到的"功能性"缺陷，不是观感问题。**

`BaseModal.vue:247-249` 现状：

```css
/* visibility 一起参与过渡：淡出期间就不会再挡住底层的点击 */   ← 注释的承诺
transition: opacity var(--dur) var(--ease), visibility var(--dur) var(--ease);   ← CSS 的实际行为
```

`visibility` 是**离散可过渡属性**，插值规则特殊：渐隐时它在**整个过渡结束时**才跳到 `hidden`，渐显时则**立即**变 `visible`。所以关闭动画进行的那 `--dur`（220ms）里，`.overlay` 的 `visibility` 仍然是 `visible`、`position: fixed; inset: 0`、`z-index: 1000` —— **它依然是一个铺满全屏的可点击层**。

用户看到的现象：点关闭 → 面板开始淡出 → 立刻去点别处（比如列表里下一篇文章）→ **第一下点空**。用户不会归因为"遮罩过渡"，只会觉得「这界面反应慢半拍」。**这正是"交互不流畅"里最难自查的一类：它没有视觉证据，而代码注释还在告诉你已经处理过了。**

**修法已在 §3.2.2 里**（`pointer-events: none` 于基础态、`auto` 于 `.show`），这里只把病因记下来，因为：

- 只改 `visibility` 的过渡时长（220ms → 120ms）**不会**修好它，只会让点击被吞的时间**变短**——看起来像修好了，其实是变短了。这类"症状减轻 ≠ 病因消除"最容易在验收时被放过。
- 那句注释是**错的**，后续任何人读到它都会以为这里没问题。改 CSS 时请**连注释一起改**，否则下一个人会把 `pointer-events` 当成冗余代码删掉。

---

## 3. 提案

### 3.1 运动尺度：4 档，按用途定义（**给死值**）

新增 4 个 token，挂在 `.overlay` 上（`BaseModal` 的根元素，自定义属性会继承给面板内所有子元素）。**不重定义 `--dur` / `--ease`**——它们被弹窗外 30 余处使用，改值会波及全文界面，而用户明确说"不是整体界面呆板"。

| 档 | token | 值 | 用途 | 为什么是这个数 |
|---|---|---|---|---|
| T1 即时 | `--m-tap` | **90ms** | 悬停、按下、焦点环出现、chip 选中 | 100ms 以下人感知为"立刻但有过渡"；150ms 已经能听出"慢半拍"。悬停是全应用最高频的交互，它必须比"面板进出"快，不能共用 |
| T2 状态 | `--m-state` | **160ms** | tab 换页签、筛选项选中、开关、折叠展开（小面积） | 介于即时与面板之间；这类变化伴随内容变化，太快会像"跳帧"，太慢会像"在加载" |
| T3 面板进 | `--m-in` | **200ms** | 弹窗进场 | 200ms 是"物体出现"的常用上限；再长会觉得界面在磨蹭。比现在的 220ms 短 20ms，同时换曲线（下方） |
| T4 面板出 | `--m-out` | **120ms** | 弹窗退场 | **退场必须比进场快**（120 / 200 = 0.6×）。人已经在离开，慢退场会被读成"卡住"。120ms 短到"干净"，又不至于变成瞬切 |

**两条曲线（新增，不改 `--ease`）**：

```css
--m-ease-out: cubic-bezier(0.2, 0.8, 0.2, 1);   /* 进场 / 悬停 / 状态：快起慢停 */
--m-ease-in:  cubic-bezier(0.4, 0, 1, 1);       /* 退场：慢起快走，末段加速离开 */
```

- 进场用 `ease-out`：物体进入视野应当**先快后慢**（像被放下），现有 `--ease`(0.4,0,0.2,1) 是两端缓，起手会"起不来"。
- 退场用 `ease-in`：离开应当**越走越快**，末段不留尾巴。这正是 §2.1 说的"拖一下才没"的解药。
- 不用弹簧 / 不用 overshoot：`cubic-bezier` 里 `y` 超过 1 会产生回弹，弹窗是"工具"不是"玩具"，回弹会削弱可信度。

### 3.2 进出场完整规格

#### 3.2.1 要改的三件事

| # | 位置 | 现在 | 改成 |
|---|---|---|---|
| 1 | `.overlay` 过渡（`BaseModal.vue:252`） | `opacity var(--dur) var(--ease), visibility var(--dur) var(--ease)` | 拆成开/关两套：开 `opacity var(--m-in) var(--m-ease-out), visibility 0s`；关 `opacity var(--m-out) var(--m-ease-in), visibility 0s linear var(--m-out)` |
| 2 | 遮罩拦截 | 靠 `visibility` 过渡在末端才变隐藏，淡出期间仍挡住点击 | 关闭态立刻 `pointer-events: none`（`.overlay` 基础态就是 `none`，`.show` 才 `auto`） |
| 3 | 面板的进/出（`:290-292` 的 `animation` 声明 + `:293-302` 的 `@keyframes base-modal-in`） | `animation: base-modal-in var(--dur) var(--ease)`，**只有进场** | **整段删掉**，改成 `.base-modal` 与 `.overlay.show .base-modal` 上一对 `transition`（下方 CSS）。退场不需要新增代码，它是第 3 条的**顺带结果** |

第 3 条是这次唯一的机制改动，值得说明白：

**现在的问题不是"少写了一段退场动画"，是"退场时动画声明整条消失"。** `animation` 挂在 `.overlay.show .base-modal` 上（`:290-292`），关弹窗 = 删掉 `.show` = 这条声明不再命中 = 元素**立刻回到 base 态**（`opacity: 1` / 无 `transform`）。这不是"反向播放"，是"取消播放"。

换成 `transition` 之后，**base 态本身就承担了"退场终点"的角色**：撤掉 `.show` 是一次普通的属性变化，有前值（`opacity: 1`）可比，于是 120ms 内平滑走回 `opacity: 0` / `translateY(6px)`。同一份代码同时定义了两端，**不存在"忘记写退出"这种失败模式**——即使过渡因为某种原因没跑，元素也只会停在 `opacity: 0`（不可见），而不是像今天这样"啪"地停在可见态。

#### 3.2.2 可以直接照抄的 CSS

```css
/* ── 遮罩：基础态 = 关闭态。pointer-events 立刻失效，避免淡出期间挡住底层点击 ── */
.overlay {
  --m-in: 200ms; --m-out: 120ms; --m-tap: 90ms; --m-state: 160ms;
  --m-ease-out: cubic-bezier(0.2, 0.8, 0.2, 1);
  --m-ease-in: cubic-bezier(0.4, 0, 1, 1);

  opacity: 0;
  visibility: hidden;
  pointer-events: none;
  transition: opacity var(--m-out) var(--m-ease-in),
              visibility 0s linear var(--m-out);
}
.overlay.show {
  opacity: 1;
  visibility: visible;
  pointer-events: auto;
  transition: opacity var(--m-in) var(--m-ease-out), visibility 0s;
}

/* ── 面板：用 transition 表达进/出，取代原来的 animation ──
   基础态 = 关闭态（退场终点）；.show 态 = 打开态（进场终点）。 */
.base-modal {
  opacity: 0;
  transform: translateY(6px);
  transition: opacity var(--m-out) var(--m-ease-in),
              transform var(--m-out) var(--m-ease-in);
}
.overlay.show .base-modal {
  opacity: 1;
  transform: translateY(0);
  transition: opacity var(--m-in) var(--m-ease-out),
              transform var(--m-in) var(--m-ease-out);
}
```

同时**删掉**这两段（不再需要）：`:290-292` 的 `.overlay.show .base-modal { animation: base-modal-in var(--dur) var(--ease) }` 和 `:293-302` 的 `@keyframes base-modal-in`。

四条要点（另加第 0 条关于注释）：

0. **`BaseModal.vue:247` 那句注释必须跟着 CSS 一起改掉。** 现状写的是「visibility 一起参与过渡：淡出期间就不会再挡住底层的点击」——**这句与实际行为相反**（`visibility` 渐隐时在整个过渡结束时才跳 `hidden`，所以那 220ms 里遮罩仍然是可点击的全屏层）。病因见 §2.8。改完 `pointer-events` 之后，那句注释应改成「`pointer-events` 基础态为 `none`：关闭瞬间即让出点击，不依赖 `visibility` 的过渡时序」——**不要改成"删掉"**，因为正是它解释了这行 `pointer-events` 为什么不能删。

1. **6px 是"进场与退场共用同一个起点/终点"的必然结果，取 6 而不是 8 是折中。** 基础态只有一个 `translateY` 值，它同时是进场起点和退场终点——**想做到"8px 进、4px 出"的不对称，就必须上 JS**（§3.2.3 备选 B，不建议）。6px 保留了大部分"被放下"的距离感，同时把 §4.1 的模糊重算位移比 8px 再少 25%。

2. **`visibility 0s linear var(--m-out)` 是关键，且必须与面板退场时长严格相等。** 关闭时遮罩要立刻开始淡出，但 `visibility` 必须**在淡出结束后**才变 `hidden`，否则面板会在第一帧就消失（`visibility` 是离散属性，没有中间值）。这里两者都读同一个 `--m-out`，**天然同步、不会漂移**——这是零 JS 方案额外拿到的收益（原方案得在 JS 里手抄一遍 120）。这条与现有 `transition: ... visibility var(--dur)` 的**意图相同、写法更明确**，不打架。

3. **不要给 `.base-modal` 再加 `animation`。** 进/出已经由 `transition` 表达，再叠一层 `animation` 会抢同一组属性（`opacity` / `transform`），出现"过渡走到一半被动画覆盖"的闪烁。**`.base-modal` 这一层只用 `transition`。**

4. **基础态的 `transform: translateY(6px)` 现在必须留在 `.base-modal` 上。** 已核实它不与居中打架：`.overlay` 用 flex 居中（`:242-244` `display: flex` + `align-items/justify-content: center`），`.base-modal`（`:265-287`）自己没有 `transform`。**将来若有人把居中改成 `transform: translate(-50%, -50%)`，必须把这行合并进去**，否则退场会跳到左上角——这是这个方案唯一的"埋雷点"，已写进 §4.3。

#### 3.2.3 零 JS：为什么这次不需要动 `script`

**结论：`BaseModal.vue` 的 `<script setup>` 一行都不用改。** 上面那对 `transition` 完全靠 `.show` 的加/撤驱动，`watch(() => props.open, ...)`（`:230`）保持原样。

这条是与并行推进的 `modal-impl-checklist.md` 对账后改的。那份清单在 `:355` 的结论是「**不建议**为此在 `BaseModal` 里加 `closing` 状态——面板常驻、`watch(open)` 已有激活/去激活逻辑，多一个状态就多一处"快速连点 / 跨零点"的边界」。**这个判断是对的，我采纳，原稿的 `.overlay.closing` + 4 行 JS 方案（含 `setTimeout`、`EXIT_MS` 常量、`clearTimeout` 守卫）整段删除。**

采纳前我核实了它依赖的前提，三条都成立：

- **12 个 `BaseModal` 消费方全部用 `:open="..."` 传 prop，没有任何一个 `<BaseModal v-if>`。**（`FeedAddModal.vue:57` 的注释直接写明「BaseModal 常驻挂载（关闭只是隐藏而非卸载）」。）→ 面板始终在 DOM 里，`.show` 是唯一的变量。
- **因此 `transition` 一定有前值可比**：撤 `.show` 是"`opacity: 1` → `opacity: 0`"的可见变化，过渡必然触发，不需要任何"我正在关闭"的状态位去把动画"挂住"。
- **`.base-modal` 现有 0 条 `transition`**（`:265-287` 只有 `position/display/max-*/background/backdrop-filter/border/border-radius/box-shadow/overflow/outline`），新增一条不会与既有声明互相覆盖。

**代价，写明白：挂载时 `open` 已经是 `true` 的情况，不再播进场。** `transition` 需要前值，元素首帧就处在 `.show` 态，没有"从 0 到 1"的过程。当前代码里这个场景不存在（上面第一条）；`BaseModal.vue:229` 那句注释提到「父组件用 v-if 控制时」可能出现——那是**防御性注释，不是现状**。

**备选 B（只在确需"8px 进、4px 出"不对称位移时才用，不要默认这么做）**：加一个 `closing` class 走 `animation`，并保留 `if (locked)` 守卫防首挂闪一下：

```ts
const closing = ref(false)
watch(() => props.open, (isOpen) => {
  if (isOpen) { closing.value = false; activate() }
  else {
    if (locked) { closing.value = true; setTimeout(() => (closing.value = false), EXIT_MS) }
    deactivate()   // 仍在同一 tick：动效只延迟视觉，不延迟行为
  }
}, { immediate: true })
```

`EXIT_MS = 120` 必须与 `--m-out` 同源（这是备选 B 唯一的耦合成本，也是不建议它的第二个理由）。`deactivate()`（`:212-227`：移除键盘监听、`releaseDialog()`、归还焦点）**绝不能挪进 `setTimeout`**——否则那 120ms 里焦点还停在一个"看起来正在消失"的弹窗内，键盘用户会撞墙。另外两点：`locked` 是普通 `let`（`:93`）不是 `ref`，不要为这段代码把它改成响应式（会牵动 `activate`/`deactivate` 全部读写点）；`setTimeout` 的句柄要清，但**不要再加一条 `onBeforeUnmount`**——`:234` 已有 `onBeforeUnmount(deactivate)`，直接在那行旁边补一句 `clearTimeout(...)` 即可，避免两条卸载钩子分散。

#### 3.2.4 焦点环：一处规则，全弹窗生效

照抄仓内已有的 4 处写法（`AppSwitch.vue:112-114` 等），放进 `styles.css` 的全局段，**在 `button` 基线之后**：

```css
button:focus-visible,
[role='button']:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 2px;
  transition: none;              /* 焦点环不该有过渡：它必须"立刻出现" */
}
```

`transition: none` 是刻意的：焦点环是**状态指示**，不是装饰。加过渡会让"按 Tab 之后 90ms 才看见框"，键盘用户会连按两次。

**不要删弹窗里那 8 处 `outline: none`。** 它们全在输入框上（§2.4 已核），且各自配了 `border-color` 焦点态；全局的 `input:focus-visible`（`:690-695`）用的是 `box-shadow` 环，与组件里的 `border-color` 叠加是"边框变色 + 外圈光环"，是更好的样式，保留。

**唯一可议的一处是 `BaseModal.vue:286` 的 `.base-modal { outline: none }`。** 它是给 `tabindex="-1"` 的面板去默认框用的——但当面板内**没有任何可聚焦元素**时，`moveFocusIn()`（`:131-138`）会把焦点落到面板本身（`:136` 的 `?? root`），此时用户看不见焦点在哪。建议补一条 `.base-modal:focus-visible { outline: 2px solid var(--primary); outline-offset: -4px }`（`offset` 取负值，画在面板内侧，避免被 `overflow: hidden` 裁掉）。这是边界情况，但修它只要一行。

### 3.3 逐弹窗改动清单（文件 + 类名 + 加哪条）

前置：先做 §3.2.2（外壳）与 §3.2.4（焦点环），再逐弹窗。

| # | 文件 | 类名 / 行 | 改动 |
|---|---|---|---|
| 1 | `BaseModal.vue` | `.overlay:238-257`、`.base-modal:265-287`、删 `:290-302` | 按 §3.2.2 整段替换；**`script` 一行不改**（§3.2.3 论证过零 JS） |
| 2 | `SettingsModal.vue` | `.side-tab:201`、`.btn-icon:239` | `transition: all 0.15s` → `transition: background var(--m-tap) var(--m-ease-out), color var(--m-tap) var(--m-ease-out)`（`.btn-icon` 另加 `transform`，让 `:active` 回弹有过渡） |
| 3 | `ResearchModal.vue` | `.side-tab:179`、`.btn-icon:221` | 同上 |
| 4 | `EntityDashboardModal.vue` | `.tech-link:137/190`、`.tech-collapse-fab:211` | **新增** `:hover` 态（`color`/`background` 任选其一，`--m-tap`）+ `transition: color var(--m-tap) var(--m-ease-out)`。这一处是"补反馈"，不是"改时长" |
| 5 | `ShortcutsModal.vue` | `.sc-row:106` | **新增** `transition: background var(--m-tap) var(--m-ease-out)`（唯一一处真硬跳） |
| 6 | `TagPicker.vue` | `.tp-name-input:388` | `transition: border-color var(--m-tap) var(--m-ease-out)`。**`:focus` 里的 `outline: none` 保留**（它是输入框、已配 `border-color` 焦点态；§3.2.4 论证过不要删） |
| 7 | `FeedManageModal.vue` | `.manage-tab:302` | `transition: color 0.15s, border-color 0.15s` → 加 `background`，三属性都用 `var(--m-tap)` |
| 8 | `InsightsModal.vue` | `.ins-range-btn:295` | `all 0.15s` → `transition: background var(--m-tap) var(--m-ease-out), color var(--m-tap) var(--m-ease-out)` |
| 9 | `DigestModal.vue` | `.digest-article:321`、`.refresh-btn:397` | 0.15s → `var(--m-tap)`；`all` 拆成显式属性 |
| 10 | `EntityAlertRuleModal.vue` | `.arm-chip:296`、`.arm-cancel/.arm-save:348/362` | `all 0.15s` → 显式属性 + `var(--m-tap)` |
| 11 | `OnboardingModal.vue` | `:142` | `var(--dur)` → `var(--m-state)`（步骤切换属于 T2，不是 T1） |
| 12 | `GlobalModal.vue` | `.modal-close-btn:235` | 走全局环即可，无需改时长 |
| 13 | `RulesModal.vue` / `FeedAddModal.vue` | `.manage-close` / `.add-close` | 走全局环即可，无需改时长 |

**改完的统一判据（可 grep，含预期结果）：**

```bash
# ① 弹窗内不应再有 all 与裸写时长 —— 两条都必须 0 命中
grep -rn "transition: all" src/components/*Modal.vue src/components/TagPicker.vue
grep -rn "0\.15s\|0\.25s"  src/components/*Modal.vue src/components/TagPicker.vue
```

**② 全仓 `transition: all` 改完仍有 13 处，全部在弹窗外——这次不碰，但不能假装 grep 是干净的：**

`ResearchFab:366`、`ArticleListToolbar:216`、`TitleBar:284`、`TitleBar:299`、`RecommendView:370`、`FeedPanel:1398`、`ContentToolbar:426`、`ResearchEntitiesTab:323/344/361/443`、`ArticleListItem:349`、`ArticleSortBar:107`。

它们和弹窗是**同一类问题**（`all` 会连带过渡布局属性），但修它们会改动弹窗之外的界面手感，与"用户说的是弹窗"这个前提不符。**留作下一轮同一模板的独立任务**，不要顺手改——顺手改会让这次改动的回归面从"14 个弹窗"扩大到"整个应用"，收益相同、风险不同。

### 3.4 `prefers-reduced-motion`：一处覆盖，不逐处

**现状：全应用 6 处**（`CommandPalette.vue:941`、`FeedPanel.vue:1329`、`ToastHost.vue:125`、`ArticleChatPanel.vue:459`、`SelectCapsule.vue:660`、`ArticleSummaryCard.vue:149`），另有 3 处 JS 判定（`EntityGraph.vue:458`、`ArticleToc.vue:82`、`SelectCapsule.vue:344`）。**14 个弹窗一处都没有。**

注意这个分布本身说明一件事：**能力是有的，先例是有的，只是没人把它套到弹窗上**。所以落地方式不是"引入新机制"，是"补一段全局兜底"，与既有的 6 处并存（它们做的是更具体的事，比如关掉画布逐帧动画，不冲突、不要删）。

放在 `styles.css` 末尾，一处覆盖全部：

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
    scroll-behavior: auto !important;
  }
}
```

三个细节：

1. **用 `0.01ms` 不用 `0s`。** `0s` 时 `transitionend` 事件不触发，任何依赖它收尾的逻辑会卡住。本方案虽然零 JS、不依赖任何事件，但下一个人很可能在这里接一个 `transitionend`（比如做滚动位置复位），留 `0.01ms` 等于提前把这个坑填了。
2. **`!important` 在这里是正确用法。** 它要压过 `transition: all 0.15s` 这类具体规则与内联样式；这是极少数该用 `!important` 的场合。
3. **它不会让退场"不消失"。** 时长压到 0.01ms 后，`opacity` 立刻到 0；`visibility` 那条的 **duration 是 0s、delay 是 `--m-out`（120ms）**，`transition-duration` 的 `!important` 改不到 `transition-delay`，所以 `visibility` 仍在 120ms 后跳 `hidden`（元素已经全透明，不可见）。**这一点要确认**：如果你在 DevTools 里看到 reduced-motion 下"关闭后弹窗看不见、却挡住了点击"，说明 `pointer-events` 那条没落上——基础态必须是 `none`；`visibility` 的 120ms 尾巴本身不挡。

---

## 4. 性能与风险

### 4.1 本次改动里唯一可能"越改越卡"的地方：退场的位移

面板是 `backdrop-filter: blur(28px)` 的表面（`BaseModal.vue:275-276`）。**它移动 = 每一帧重新对背后内容做一次 28px 高斯模糊。**

**这就是 §3.2.2 把位移定在 6px 而不是更大的原因**，也是这个方案在性能上唯一需要盯的数：

- 进场 6px 与退场 6px 是同一段位移（base 态只有一值），**一次开关共付 6px 的模糊重算**，不是 8+8=16px。
- 如果坚持要"8px 进、4px 出"，总位移 12px，比现在的 6px 翻一倍——性能上更差，这也是 §3.2.3 不建议走备选 B 的第三个理由。

**如果低配机上仍然掉帧，按这个顺序降级（每一档都是独立的、可回退的）：**

| 降级 | 做法 | 代价 |
|---|---|---|
| L1（默认） | `translateY(6px)` + 淡出，进出共用 | — |
| L2 | 位移归零：`transform` 两行全删，只留 `opacity` | 少了方向感，但仍有"独立淡出"，比现状好；**模糊重算成本直接归 0** |
| L3 | 面板过渡整段删掉（回到今天的现状） | 只剩遮罩淡出 |

**不要**用"缩小 + 淡出"（`scale(0.96)`）：缩放会改变 `backdrop-filter` 的取样区域尺度，重算量比位移更大。

### 4.2 自查方法（DevTools 里看什么，给具体位置）

1. **Performance 面板**：录制"打开 → 等 500ms → 关闭"，看 `Frames` 行。掉帧会表现为长帧（>16.7ms）。重点看关闭段有没有连续长帧——那 120ms 里如果有 3 帧以上超 20ms，就走 L2。
2. **Rendering 面板 → Paint flashing**：悬停时如果整块面板闪绿，说明有元素在触发大面重绘（通常是漏了 `transition: all` 波及布局属性）。
3. **Layers 面板**：确认 `.base-modal` 在动画期间是独立合成层。加 `will-change: transform, opacity` 可强制提升——**但只在动画期间需要，常驻 `will-change` 会常占显存**。建议不加，先测；确实需要时用 `:hover` 不行，应在 `.overlay.show .base-modal` 上加（只在打开期间生效）。
4. **`prefers-reduced-motion`**：DevTools → Rendering → `Emulate CSS media feature prefers-reduced-motion: reduce`，开关弹窗应**完全无过渡**（且不出现"卡在中间态"）。

### 4.3 已知的三个风险点（写进来是为了让实现方别踩）

1. **两层遮罩叠加时模糊成本翻倍。** `GlobalModal` 是 `z-index: 9999` 的另一个 `.overlay`（`BaseModal.vue:260-262`），叠在普通弹窗之上时，屏幕上有 **2 个 `blur(10px)` 遮罩 + 2 个 `blur(28px)` 面板**同时存在。这不是本次引入的，但**任何让"叠层"更频繁的改动都会放大它**（例如给保底弹窗加延迟动画）。
2. **`.overlay` 上的 `visibility` 过渡容易被改坏。** 三种输入（开/关/reduced-motion）共用一条声明，谁把它简化成 `transition: opacity ...` 一处，关闭时面板就会第一帧消失。§3.2.2 那句 `visibility 0s linear var(--m-out)` 是**有意为之**，不要"整理"掉。
3. **`.base-modal` 基础态那行 `transform: translateY(6px)` 是"埋雷点"。** 它和"面板居中"共用 `transform` 这一个属性槽——现在居中靠 `.overlay` 的 flex（`:242-244`），所以安全；但**一旦有人把居中改成 `transform: translate(-50%, -50%)`（比如为了贴边悬浮），退场会直接跳到左上角**。判据：`.base-modal` 上任何新增的 `transform` 都必须与 `translateY(6px)` 合并写在同一处，不要分散两条声明。

---

## 5. 这次不做什么（都有理由，请不要顺手加）

| # | 看着该加的东西 | 为什么不做 |
|---|---|---|
| 1 | **tab 内容切换的淡入淡出** | 内容替换不是"状态渐变"。加 200ms 淡入，用户会把它读成"正在加载"——而 `SettingsModal` 的 tab 是纯本地切换，本来没有加载。**在一个没有等待的地方制造等待感，是负收益。** |
| 2 | **列表项 hover 的位移**（`translateX(2px)`） | 已经在 hover 里通过背景色表达了"这一行可点"；再加位移会让列表看起来在"抖"。而且 `DigestModal` 的整行是 `<div @click>`，位移会强化"像按钮"的错觉，与"整行进入"的语义打架。 |
| 3 | **弹窗高度变化的动画** | 需要 JS 量测（`height: auto` 无法过渡），且要与 `max-height: 86vh` + 内部滚动区共存。`layout-side` 变体（`BaseModal.vue:333-340`）之所以固定高度，就是**为了消除切 tab 时的高度跳变**——用动画去演高度变化等于把这个设计退回去。 |
| 4 | **`backdrop-filter` 的过渡** | 不可能便宜：改模糊半径 = 每帧重算全屏区域。而且是"装饰性动画"，收益为零。 |
| 5 | **给 `GlobalModal` 加"从父弹窗里缩放弹出"的层叠动效** | 两层模糊（§4.3）叠加时，任何额外动画都有双倍成本，而它的语义是"打断你去确认一件事"——越快越好，不需要仪式感。 |
| 6 | **改 `BaseModal` 的焦点陷阱 / 归还 / Esc 让路 / `mousedown` 关遮罩** | 四条都是修过 bug 的（`mousedown` 那条的理由写在 `:38-40`）。本次**只改 CSS**（`:290-302` 删、`.base-modal` 加一对 `transition`），`script` 一行不动、`:230` 的 `watch` 原样保留，**行为零变化**（§3.2.3 已标明）。 |
| 7 | **给 `TagPicker` 套 `BaseModal`** | 它是**锚定在工具条下方**的轻量浮层，不是模态：没有遮罩、不锁滚动、点外部即关。套上 `BaseModal` 会得到遮罩与焦点陷阱——那会让它变成"每次打标签都要先关一个模态"。它需要的只是自己的 90ms 悬停过渡。 |
| 8 | **滑动指示器（tab 下划线滑过去）** | 项目里已有 `--dur-slide` / `--ease-slide` 专供 `SelectCapsule`，但 14 个弹窗里没有"同一容器内等宽切换"的语义（side tab 是竖排不定高）。硬做成本高、收益低，且会与 §5 第 1 条冲突。 |
| 9 | **加载骨架** | 本轮只做"过渡"，不做"等待表达"。`InsightsModal` 换时间范围确实有重载空窗（§1.1 #8），但那属于组件契约（加载/空/错三态）的范围，不是动效问题；混进来会让这份文档的边界失守。 |

---

## 附：本文所有事实的复核命令

```bash
# 外壳：遮罩过渡 / 面板动画 / 两处 backdrop-filter / 宽度档
sed -n '238,302p' src/components/BaseModal.vue

# 全局 button 基线（六属性 + :active 回弹）
sed -n '565,575p' src/styles.css

# 运动 token
sed -n '303,315p' src/styles.css

# 弹窗内 transition 全清单
grep -rn "transition:" src/components/*Modal.vue src/components/TagPicker.vue

# transition: all 的 8 处
grep -rn "transition: all" src/components/*Modal.vue

# 焦点环现状（全局只覆盖表单控件）
sed -n '688,696p' src/styles.css
grep -rn "focus-visible" src/components/*.vue

# reduced-motion 现有 6 处（无一处是弹窗）
grep -rn "prefers-reduced-motion" src/

# 滚动锁与滚动条宽度（4px 位移的来源）
sed -n '30,50p' src/utils/dialogStack.ts
sed -n '544,550p' src/styles.css

# 零 JS 方案的前提：12 个消费方全部 :open 传 prop，无一处 <BaseModal v-if>
grep -rln "from './BaseModal.vue'" src/components/*.vue        # 预期 12 个文件
grep -rn -A6 "<BaseModal$" src/components/*.vue | grep "v-if"
# ↑ 实跑结果：1 命中，是 `EntityAlertRuleModal.vue:9` 的内层 `<div class="arm" v-if="entity">`，
#   属面板内容、不是外壳属性。**没有一处 v-if 挂在外壳上**——这是零 JS 方案成立的前提。

# .base-modal 自己没有 transform（居中靠 .overlay 的 flex），故基础态可安全写 translateY
grep -n "transform" src/components/BaseModal.vue               # 预期只命中 :296 / :300 两处 keyframes 内
sed -n '242,244p' src/components/BaseModal.vue                 # display:flex + align/justify center
```
