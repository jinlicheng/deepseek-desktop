import { createApp } from "vue";
import { invoke } from "@tauri-apps/api/core";
import App from "./App.vue";
import "./styles.css";

const app = createApp(App);

// Vue 默认会吞掉渲染错误（表现为面板/区域空白），这里回报给 Rust 打印到终端，便于排查
app.config.errorHandler = (err, _instance, info) => {
  const message = err && err.message ? err.message : String(err);
  invoke("report_ui_error", { message: `${info}: ${message}` }).catch(() => {});
};

app.mount("#app");
