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
// 提示条：toast 保存内容（滑出期间保留，避免淡出时变空），toastShown 控制显隐
const toast = ref(null);
const toastShown = ref(false);
let unlisten = null;
let unlistenDownload = null;
let toastTimer = null;

onMounted(async () => {
  // 上报真实视口高度：Rust 侧据此校准 macOS 标题栏偏移（子 webview 的定位基准）
  reportViewport();
  state.value = await invoke("get_state");
  unlisten = await listen("state-changed", (event) => {
    state.value = event.payload;
  });
  unlistenDownload = await listen("download-finished", (event) => {
    const { name, success } = event.payload;
    showToast(success ? "下载完成" : "下载失败", success, name);
  });
  window.addEventListener("resize", reportViewport);
  window.addEventListener("keydown", onKeydown);
});

onUnmounted(() => {
  if (unlisten) unlisten();
  if (unlistenDownload) unlistenDownload();
  clearTimeout(toastTimer);
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

/// 顶部滑入的下载提示：2.6 秒后自动收起（内容保留，等淡出结束）
function showToast(text, success = true, detail = "") {
  toast.value = { text, success, detail };
  toastShown.value = true;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    toastShown.value = false;
  }, 2600);
}

function hideToast() {
  clearTimeout(toastTimer);
  toastShown.value = false;
}

/// 「立即查看」：在文件管理器中定位最近一次下载的文件
function revealDownload() {
  hideToast();
  run(() => invoke("reveal_last_download"));
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
  <div
    class="toast"
    :class="{ show: toastShown, fail: toast && !toast.success }"
    :title="toast ? toast.detail : ''"
  >
    <span class="toast-icon">{{ toast && !toast.success ? "!" : "✓" }}</span>
    <span class="toast-text">{{ toast ? toast.text : "" }}</span>
    <button v-if="toast && toast.success" class="toast-action" @click="revealDownload">
      立即查看
    </button>
    <button class="toast-close" @click="hideToast">×</button>
  </div>
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
      @download-dir="(dir) => run(() => invoke('set_download_dir', { dir }))"
      @download-per-site="(enabled) => run(() => invoke('set_download_per_site', { enabled }))"
    />
  </div>
</template>
