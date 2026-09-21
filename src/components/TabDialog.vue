<script setup>
import { ref } from "vue";

const emit = defineEmits(["close", "submit"]);
const name = ref("");
const url = ref("");
const save = ref(false);

function submit() {
  emit("submit", { name: name.value.trim(), url: url.value.trim(), save: save.value });
}
</script>

<template>
  <div class="panel">
    <h2>添加标签页 <span class="x" @click="emit('close')">×</span></h2>
    <div class="field">
      <label>名称</label>
      <input v-model="name" type="text" placeholder="例如：豆包" autofocus />
    </div>
    <div class="field">
      <label>网址（可不带 https://）</label>
      <input
        v-model="url"
        type="text"
        placeholder="例如：www.doubao.com"
        @keyup.enter="submit"
      />
    </div>
    <label class="row" style="cursor: pointer">
      <input v-model="save" type="checkbox" />
      保存到配置（重启后保留为常驻标签页）
    </label>
    <div class="row">
      <button
        class="primary grow"
        :disabled="!name.trim() || !url.trim()"
        @click="submit"
      >确定</button>
      <button @click="emit('close')">取消</button>
    </div>
  </div>
</template>
