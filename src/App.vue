<script setup>
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import TabBar from "./components/TabBar.vue";
import TabDialog from "./components/TabDialog.vue";
import SettingsPanel from "./components/SettingsPanel.vue";
import AvailablePanel from "./components/AvailablePanel.vue";

const state = ref(null);
const error = ref("");
let unlisten = null;

onMounted(async () => {
  // 上报真实视口高度：Rust 侧据此校准 macOS 标题栏偏移（子 webview 的定位基准）
  reportViewport();
  state.value = await invoke("get_state");
  unlisten = await listen("state-changed", (event) => {
    state.value = event.payload;
  });
  window.addEventListener("resize", reportViewport);
  window.addEventListener("keydown", onKeydown);
});

onUnmounted(() => {
  if (unlisten) unlisten();
  window.removeEventListener("resize", reportViewport);
  window.removeEventListener("keydown", onKeydown);
});

function reportViewport() {
  invoke("report_viewport", { innerH: window.innerHeight });
}

function onKeydown(e) {
  // Esc 关闭面板（输入框聚焦时不触发）
  if (e.key === "Escape" && !["INPUT", "TEXTAREA"].includes(e.target.tagName)) {
    run(() => invoke("set_panel", { panel: null }));
  }
}

/// 三个入口按钮统一为「点击切换」：同一个面板已打开时再点就关闭
function togglePanel(kind) {
  run(() => invoke("set_panel", { panel: state.value.panel === kind ? null : kind }));
}

async function run(action) {
  error.value = "";
  try {
    await action();
  } catch (e) {
    error.value = String(e);
  }
}
</script>

<template>
  <TabBar
    v-if="state"
    :state="state"
    :error="error"
    @switch="(id) => run(() => invoke('switch_tab', { id }))"
    @close="(id) => run(() => invoke('close_tab', { id }))"
    @reload="() => state.active && run(() => invoke('reload_tab', { id: state.active }))"
    @add="() => togglePanel('add')"
    @toggle-available="() => togglePanel('available')"
    @settings="() => togglePanel('settings')"
  />
  <div id="content">
    <div class="placeholder">
      {{ state && state.open.length === 0
        ? "还没有打开的标签页，点右上角「＋」添加一个"
        : "页面加载中…" }}
    </div>
    <AvailablePanel
      v-if="state && state.panel === 'available'"
      :state="state"
      @close="() => run(() => invoke('set_panel', { panel: null }))"
      @open="(id) => run(() => invoke('open_available', { id }))"
    />
    <TabDialog
      v-if="state && state.panel === 'add'"
      @close="() => run(() => invoke('set_panel', { panel: null }))"
      @submit="(form) => run(() => invoke('add_tab', form))"
    />
    <SettingsPanel
      v-if="state && state.panel === 'settings'"
      :state="state"
      @close="() => run(() => invoke('set_panel', { panel: null }))"
      @update="(id, patch) => run(() => invoke('update_tab', { id, ...patch }))"
      @delete="(id) => run(() => invoke('delete_tab', { id }))"
      @move="(id, up) => run(() => invoke('move_tab', { id, up }))"
      @startup="(n) => run(() => invoke('set_startup_count', { n }))"
    />
  </div>
</template>
