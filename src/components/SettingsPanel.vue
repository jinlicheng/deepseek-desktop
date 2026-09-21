<script setup>
import { reactive, watch } from "vue";

const props = defineProps({ state: Object });
const emit = defineEmits(["close", "update", "delete", "move", "startup"]);

// 每行的草稿：输入过程不触发写盘，点「✓ 保存」才提交
const drafts = reactive({});
watch(
  () => props.state.tabs,
  (tabs) => {
    for (const t of tabs) {
      if (!drafts[t.id]) drafts[t.id] = { name: t.name, url: t.url };
    }
    for (const key of Object.keys(drafts)) {
      if (!tabs.some((t) => t.id === key)) delete drafts[key];
    }
  },
  { immediate: true }
);
</script>

<template>
  <div class="panel">
    <h2>设置 <span class="x" @click="emit('close')">×</span></h2>

    <div class="field">
      <label>启动时自动打开前 {{ state.startup_count }} 个标签</label>
      <input
        type="number"
        min="0"
        :max="Math.min(9, state.tabs.length)"
        :value="state.startup_count"
        @change="(e) => emit('startup', Number(e.target.value))"
      />
    </div>

    <div v-if="state.tabs.length === 0" class="hint">
      配置列表为空。点标签栏上的「＋」添加站点，勾选「保存到配置」即可常驻。
    </div>

    <div v-else class="hint">
      配置列表（顺序即启动顺序，前 N 个自动打开，其余收在「▾」里）：
    </div>

    <div v-for="(t, i) in state.tabs" :key="t.id" class="field" style="gap: 6px">
      <div class="row">
        <input v-model="drafts[t.id].name" type="text" style="width: 110px" />
        <input v-model="drafts[t.id].url" type="text" class="grow" />
      </div>
      <div class="row">
        <button :disabled="i === 0" @click="emit('move', t.id, true)">↑</button>
        <button :disabled="i === state.tabs.length - 1" @click="emit('move', t.id, false)">↓</button>
        <span class="grow" />
        <button
          :disabled="drafts[t.id].name === t.name && drafts[t.id].url === t.url"
          @click="emit('update', t.id, { name: drafts[t.id].name, url: drafts[t.id].url })"
        >✓ 保存</button>
        <button class="danger" @click="emit('delete', t.id)">删除</button>
      </div>
    </div>

    <div class="hint">添加标签页请点标签栏上的「＋」。</div>
  </div>
</template>
