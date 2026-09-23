<script setup>
import { reactive, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";

const props = defineProps({ state: Object });
const emit = defineEmits([
  "close",
  "update",
  "delete",
  "move",
  "startup",
  "download-dir",
  "download-per-site",
]);

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

async function chooseDownloadDir() {
  const picked = await open({
    directory: true,
    multiple: false,
    title: "选择下载目录",
    defaultPath: props.state.download_dir || undefined,
  });
  if (typeof picked === "string" && picked) emit("download-dir", picked);
}
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
      配置列表为空。点标签栏上的「＋」添加站点，勾选「保存到配置」即可留到下次启动。
    </div>

    <div v-else class="hint">
      配置列表（顺序即启动顺序，启动时自动打开前 N 个，其余收在「▾」里；拖拽标签栏只改当前顺序，这里的 ↑↓ 改的是启动顺序）：
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

    <div class="hint" style="margin-top: 6px">下载（所有站点共用）：</div>

    <div class="field">
      <label>下载目录</label>
      <div class="row">
        <span class="grow path" :title="state.download_dir || ''">
          {{ state.download_dir || "系统默认下载目录" }}
        </span>
        <button @click="chooseDownloadDir">选择…</button>
        <button v-if="state.download_dir" @click="emit('download-dir', null)">默认</button>
      </div>
    </div>

    <label class="row" style="cursor: pointer">
      <input
        type="checkbox"
        :checked="state.download_per_site"
        @change="(e) => emit('download-per-site', e.target.checked)"
      />
      按站点建立子目录
    </label>
  </div>
</template>
