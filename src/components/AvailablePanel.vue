<script setup>
defineProps({ state: Object });
const emit = defineEmits(["close", "open"]);
</script>

<template>
  <div class="panel">
    <h2>已配置的站点 <span class="x" @click="emit('close')">×</span></h2>
    <div v-if="state.available.length === 0" class="hint">
      没有更多已配置的站点了。点标签栏上的「＋」可以添加。
    </div>
    <div
      v-for="t in state.available"
      :key="t.id"
      class="avail-item"
      :title="t.url"
      @click="emit('open', t.id)"
    >
      <span>{{ t.name }}</span>
      <small>{{ t.url }}</small>
    </div>
  </div>
</template>
