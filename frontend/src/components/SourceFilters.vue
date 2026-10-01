<script setup lang="ts">
import type { Translator } from "../core/i18n";
import type { Source, SourceSelection } from "../core/types";
const props = defineProps<{ sources: Source[]; selected: SourceSelection; t: Translator }>();
const emit = defineEmits<{ select: [value: SourceSelection] }>();
function toggle(id: string, checked: boolean) {
  const selected = new Set(props.selected ?? props.sources.map((source) => source.id));
  if (checked) selected.add(id);
  else selected.delete(id);
  emit("select", [...selected].sort());
}
</script>
<template>
  <fieldset class="source-filters" aria-describedby="source-selection-help">
    <legend>{{ t("filterSources") }}</legend>
    <div class="source-filter-actions">
      <p id="source-selection-help">{{ selected === null ? t("allSourcesIncluded") : t("selectedSources", { count: selected.length }) }}</p>
      <div>
        <button type="button" @click="emit('select', null)">{{ t("selectAll") }}</button>
        <button type="button" @click="emit('select', [])">{{ t("clearSources") }}</button>
      </div>
    </div>
    <details v-if="sources.length" class="source-disclosure">
      <summary>{{ t("chooseSources", { count: sources.length }) }}</summary>
      <div class="source-options">
      <label v-for="source in sources" :key="source.id">
        <input type="checkbox" :value="source.id" :checked="selected === null || selected.includes(source.id)"
          @change="toggle(source.id, ($event.target as HTMLInputElement).checked)" />
        <span>{{ source.name }}</span>
      </label>
      </div>
    </details>
    <p v-else class="source-catalog-status">{{ t("sourceCatalogPending") }}</p>
    <p v-if="selected?.length === 0" class="source-selection-empty" role="status">{{ t("noSourcesHint") }}</p>
  </fieldset>
</template>
