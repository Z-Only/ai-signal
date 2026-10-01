import { createApp } from "vue";
import App from "./App.vue";
import "./style.css";
export function bootstrap(
  target: Element | null = document.querySelector("#app"),
) {
  if (!target) return null;
  const app = createApp(App);
  app.mount(target);
  return app;
}
export const application = bootstrap();
