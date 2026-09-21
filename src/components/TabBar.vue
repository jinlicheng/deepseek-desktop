<script setup>
defineProps({ state: Object, error: String });
const emit = defineEmits([
  "switch",
  "close",
  "toggle-available",
  "add",
  "settings",
  "reload",
]);
</script>

<template>
  <div id="tabbar">
    <div
      v-for="tab in state.open"
      :key="tab.id"
      class="tab"
      :class="{ active: tab.id === state.active }"
      :title="tab.url"
      @click="emit('switch', tab.id)"
    >
      <span>{{ tab.name }}</span>
      <span
        v-if="!tab.pinned"
        class="close"
        title="关闭"
        @click.stop="emit('close', tab.id)"
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
