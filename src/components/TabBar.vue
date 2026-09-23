<script setup>
import { ref } from "vue";

const props = defineProps({ state: Object, error: String });
const emit = defineEmits([
  "switch",
  "close",
  "toggle-available",
  "add",
  "settings",
  "reload",
  "reorder",
]);

// 拖拽排序：指针移动超过阈值才算拖拽，否则仍是"点击切换标签"
const draggingId = ref(null);
const dropIndex = ref(-1);
let startX = 0;

function tabsInBar() {
  return [...document.querySelectorAll("#tabbar .tab")];
}

/// 指针位置对应的插入位置（0 ~ 标签数）
function slotAt(x, y) {
  const tabs = tabsInBar();
  const hit = document.elementFromPoint(x, y);
  const tab = hit && hit.closest ? hit.closest(".tab") : null;
  if (!tab) return x < 80 ? 0 : tabs.length;
  const index = tabs.indexOf(tab);
  const rect = tab.getBoundingClientRect();
  return x > rect.left + rect.width / 2 ? index + 1 : index;
}

/// 把 fromId 挪到 slot 位置，返回新的 id 顺序（没变则返回 null）
function reorderedIds(fromId, slot) {
  const ids = props.state.open.map((t) => t.id);
  const from = ids.indexOf(fromId);
  if (from < 0) return null;
  const moved = ids.splice(from, 1)[0];
  const to = Math.max(0, Math.min(ids.length, slot > from ? slot - 1 : slot));
  if (to === from) return null;
  ids.splice(to, 0, moved);
  return ids;
}

function onPointerDown(tab, event) {
  if (event.button !== 0) return;
  startX = event.clientX;
  let moved = false;

  const onMove = (ev) => {
    if (!moved && Math.abs(ev.clientX - startX) < 5) return;
    moved = true;
    draggingId.value = tab.id;
    dropIndex.value = slotAt(ev.clientX, ev.clientY);
  };
  const onUp = (ev) => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    const wasDragging = moved;
    const slot = slotAt(ev.clientX, ev.clientY);
    draggingId.value = null;
    dropIndex.value = -1;
    if (!wasDragging) {
      emit("switch", tab.id);
      return;
    }
    const ids = reorderedIds(tab.id, slot);
    if (ids) emit("reorder", ids);
  };

  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
}
</script>

<template>
  <div id="tabbar">
    <div
      v-for="(tab, i) in state.open"
      :key="tab.id"
      class="tab"
      :class="{
        active: tab.id === state.active,
        dragging: draggingId === tab.id,
        'drop-before': dropIndex === i,
        'drop-after': dropIndex === state.open.length && i === state.open.length - 1,
      }"
      :title="tab.url"
      @pointerdown="onPointerDown(tab, $event)"
    >
      <span>{{ tab.name }}</span>
      <span
        class="close"
        :class="{ disabled: state.open.length <= 1 }"
        :title="state.open.length <= 1 ? '至少保留一个标签页' : '关闭'"
        @pointerdown.stop
        @click.stop="state.open.length > 1 && emit('close', tab.id)"
      >×</span>
    </div>
    <div class="spacer" />
    <div v-if="error" class="error">{{ error }}</div>
    <div class="btn" title="刷新当前标签页" @click="emit('reload')">⟳</div>
    <div class="btn" title="添加标签页" @click="emit('add')">＋</div>
    <div
      class="btn"
      :class="{ active: state.panel === 'available' }"
      title="打开已配置的站点"
      @click="emit('toggle-available')"
    >▾</div>
    <div
      class="btn"
      :class="{ active: state.panel === 'settings' }"
      title="设置"
      @click="emit('settings')"
    >⚙</div>
  </div>
</template>
