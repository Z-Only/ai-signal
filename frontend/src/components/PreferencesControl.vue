<script setup lang="ts">
import type { Translator } from "../core/i18n";
import type { Locale, Theme } from "../core/types";
defineProps<{ locale: Locale; theme: Theme; t: Translator }>();
const emit = defineEmits<{ locale: [value: Locale]; theme: [value: Theme] }>();
</script>
<template>
  <div class="preferences">
    <label
      ><span class="sr-only">{{ t("language") }}</span
      ><select
        :value="locale"
        @change="
          emit('locale', ($event.target as HTMLSelectElement).value as Locale)
        "
      >
        <option value="zh-CN" lang="zh-CN">中文</option>
        <option value="en" lang="en">English</option>
      </select></label
    >
    <label
      ><span class="sr-only">{{ t("theme") }}</span
      ><select
        :value="theme"
        @change="
          emit('theme', ($event.target as HTMLSelectElement).value as Theme)
        "
      >
        <option value="system">{{ t("system") }}</option>
        <option value="light">{{ t("light") }}</option>
        <option value="dark">{{ t("dark") }}</option>
      </select></label
    >
  </div>
</template>
