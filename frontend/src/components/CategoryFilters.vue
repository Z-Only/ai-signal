<script setup lang="ts">
import { categoryLabel, type Translator } from "../core/i18n";
import { categories, type Category, type Locale } from "../core/types";
defineProps<{
  active: Category;
  count: number;
  locale: Locale;
  t: Translator;
}>();
const emit = defineEmits<{ select: [value: Category] }>();
</script>
<template>
  <div class="tabs" role="group" :aria-label="t('categories')">
    <button
      v-for="category in categories"
      :key="category"
      type="button"
      :aria-pressed="active === category"
      :class="{ active: active === category }"
      @click="emit('select', category)"
    >
      {{ categoryLabel(locale, category)
      }}<span v-if="category === '全部资讯'">{{ count }}</span>
    </button>
  </div>
</template>
