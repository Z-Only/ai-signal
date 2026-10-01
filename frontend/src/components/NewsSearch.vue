<script setup lang="ts">
import { computed, ref, watch } from "vue";
import type { Translator } from "../core/i18n";
const props = defineProps<{ query: string; t: Translator }>();
const emit = defineEmits<{ search: [value: string] }>();
const draft = ref(props.query);
const tooLong = computed(() => Array.from(draft.value.trim()).length > 200);
const hasNull = computed(() => draft.value.includes("\0"));
const invalid = computed(() => tooLong.value || hasNull.value);
watch(
  () => props.query,
  (value) => {
    draft.value = value;
  },
);
function submit() {
  if (invalid.value) return;
  draft.value = draft.value.trim();
  emit("search", draft.value);
}
function clear() {
  draft.value = "";
  emit("search", "");
}
</script>
<template>
  <form
    class="news-search"
    role="search"
    :aria-label="t('searchLabel')"
    @submit.prevent="submit"
  >
    <label for="news-search-input">{{ t("searchLabel") }}</label>
    <div class="search-controls">
      <input
        id="news-search-input"
        v-model="draft"
        type="search"
        name="q"
        :placeholder="t('searchPlaceholder')"
        :aria-invalid="invalid"
        :aria-describedby="invalid ? 'search-help search-error' : 'search-help'"
      />
      <button class="search-submit" type="submit" :disabled="invalid">
        {{ t("search") }}
      </button>
      <button type="button" :disabled="!draft && !query" @click="clear">
        {{ t("clearSearch") }}
      </button>
    </div>
    <p id="search-help" class="search-help">{{ t("searchHelp") }}</p>
    <p v-if="invalid" id="search-error" class="search-error" role="alert">
      {{ t(tooLong ? "searchTooLong" : "searchNull") }}
    </p>
  </form>
</template>
