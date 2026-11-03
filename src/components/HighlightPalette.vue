<template>
  <!-- 高亮调色板：在正文里选中文字后浮出。刻意做成"工具条下方居中的浮层"
       而不是跟随鼠标的浮标——选区可能在任意位置，固定位置不会挡住所选内容，
       也不必处理滚动导致的定位漂移。

       `@mousedown.prevent` 是必需的：点色块会先触发一次 mousedown，浏览器默认会在
       那一刻折叠文档选区，等到 click 再读 `window.getSelection()` 往往已经空了
       （选区快照虽已在 mouseup 时存好，但阻止折叠能少一条依赖）。
       内部按钮都用 `@click.stop`：不让点击冒泡到 document 的浮层外点关闭逻辑。 -->
  <div class="highlight-palette" @mousedown.prevent>
    <span class="palette-hint">高亮</span>
    <button
      v-for="c in colors"
      :key="c.key"
      class="palette-swatch"
      :class="`swatch-${c.key}`"
      :title="c.label"
      @click.stop="emit('pick', c.key)"
    ></button>
    <!-- 右侧扩展位：「挂到论点」入口由宿主 ContentColumn 注入。
         用插槽而不是把论点逻辑塞进来：调色板只认颜色，不认识论点域。 -->
    <span v-if="$slots.extra" class="palette-sep" aria-hidden="true"></span>
    <slot name="extra"></slot>
    <button class="palette-cancel" @click.stop="emit('cancel')">取消</button>
  </div>
</template>

<script setup lang="ts">
/**
 * 高亮调色板（正文里选中文字后浮出的四色选择器）
 *
 * # 职责
 * 纯展示：把可选颜色渲染成一排色块，把"选了哪一色 / 取消"抛给父组件。
 * 颜色清单由父组件传入而非常量内联——它属于"高亮功能的可用色板"这一业务约定，
 * 与调色板的外观无关。
 *
 * # 为什么 @mousedown.prevent 必须留在这里
 * 阻止 mousedown 默认行为是"点色块时保住选区"的关键，属于本组件的交互契约，
 * 不能上提到父容器（父容器还包着别的元素，一律 prevent 会误伤文本选择）。
 */

defineProps<{
  /** 可选的高亮色板（key 同时决定色块样式类 `swatch-{key}`） */
  colors: { key: string; label: string }[]
}>()

const emit = defineEmits<{
  /** 选中某个颜色，携带其 key */
  (e: 'pick', key: string): void
  /** 取消本次高亮 */
  (e: 'cancel'): void
}>()
</script>

<style scoped>
.highlight-palette {
  position: absolute;
  /* 落在工具条（约 44px 高）下方，避免遮住已读/收藏/译文等按钮 */
  top: 56px;
  left: 50%;
  transform: translateX(-50%);
  z-index: 100;
  display: flex;
  align-items: center;
  gap: var(--sp-05);
  padding: var(--sp-05) var(--sp-15);
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-hover);
}
.palette-hint { font-size: var(--fs-sm); color: var(--text-tertiary); }
/* 扩展位与色板之间的竖分隔线 */
.palette-sep {
  width: 1px;
  height: 16px;
  background: var(--border);
  margin: 0 var(--sp-1);
}
.palette-swatch {
  width: 20px;
  height: 20px;
  border-radius: 50%;
  border: 2px solid transparent;
  cursor: pointer;
  /* 只过渡描边：色板自己是"可选的颜色"，不该在悬停时变形 */
  transition: border-color var(--dur) var(--ease);
}
/* 四档底色取自全局令牌：随主题切换，组件内不写具体色值 */
.swatch-yellow { background: var(--hl-yellow); }
.swatch-green { background: var(--hl-green); }
.swatch-blue { background: var(--hl-blue); }
.swatch-pink { background: var(--hl-pink); }
/* 悬停只把描边加重一档。原先是放大到 1.2 倍——四个色板同时可悬停，
   每个都跳一下，读作"这几个东西很不稳定"。 */
.palette-swatch:hover { border-color: var(--text-tertiary); }
.palette-swatch:focus-visible {
  outline: 2px solid var(--primary);
  outline-offset: 1px;
}
.palette-cancel {
  border: none;
  background: none;
  cursor: pointer;
  font-size: var(--fs-sm);
  color: var(--text-tertiary);
  padding: var(--sp-1) var(--sp-05);
  font-family: inherit;
}
.palette-cancel:hover { color: var(--text-primary); }
</style>
