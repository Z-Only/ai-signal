import { computed, onUnmounted, ref, watch } from "vue";
import { translate, type MessageKey } from "../core/i18n";
import type { Locale, Theme } from "../core/types";
function saved(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function persist(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    /* Preferences still work for this session if storage is unavailable. */
  }
}
export function usePreferences() {
  const locale = ref<Locale>(
    saved("ai-signal-language") === "en" ? "en" : "zh-CN",
  );
  const storedTheme = saved("ai-signal-theme");
  const theme = ref<Theme>(
    storedTheme === "light" || storedTheme === "dark" ? storedTheme : "system",
  );
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const systemDark = ref(media.matches);
  const onChange = (event: MediaQueryListEvent) => {
    systemDark.value = event.matches;
  };
  media.addEventListener("change", onChange);
  onUnmounted(() => media.removeEventListener("change", onChange));
  const resolvedTheme = computed(() =>
    theme.value === "system"
      ? systemDark.value
        ? "dark"
        : "light"
      : theme.value,
  );
  watch(
    resolvedTheme,
    (value) => {
      document.documentElement.dataset.theme = value;
    },
    { immediate: true },
  );
  watch(theme, (value) => persist("ai-signal-theme", value));
  watch(
    locale,
    (value) => {
      document.documentElement.lang = value;
      document.title =
        value === "en"
          ? "AI Signal · See what comes next"
          : "AI 信号 · 看见下一步";
      persist("ai-signal-language", value);
    },
    { immediate: true },
  );
  const t = (key: MessageKey, values?: Record<string, string | number>) =>
    translate(locale.value, key, values);
  return { locale, theme, t };
}
