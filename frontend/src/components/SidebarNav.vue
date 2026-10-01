<script setup lang="ts">
import type { Translator } from "../core/i18n";
import type { View } from "../core/types";
defineProps<{ view: View; sourceCount: number; t: Translator }>();
const emit = defineEmits<{ navigate: [value: View] }>();
</script>
<template>
  <aside class="rail">
    <a class="brand" href="/" :aria-label="t('home')"
      ><span class="brandmark" aria-hidden="true">ϟ</span
      ><span>{{ t("brand") }}<small>THE AI SIGNAL</small></span></a
    >
    <div class="rail-label">{{ t("observatory") }}</div>
    <nav :aria-label="t('navigation')">
      <button
        type="button"
        :class="{ selected: view === 'feed' }"
        :aria-current="view === 'feed' ? 'page' : undefined"
        @click="emit('navigate', 'feed')"
      >
        <span aria-hidden="true">◫</span>{{ t("feed") }}
      </button>
      <button
        type="button"
        :class="{ selected: view === 'sources' }"
        :aria-current="view === 'sources' ? 'page' : undefined"
        @click="emit('navigate', 'sources')"
      >
        <span aria-hidden="true">◎</span>{{ t("sources")
        }}<em>{{ String(sourceCount).padStart(2, "0") }}</em>
      </button>
    </nav>
    <div class="rail-bottom">
      <span class="pulse" aria-hidden="true"></span>{{ t("capturing")
      }}<small>{{ t("principles") }}</small>
    </div>
  </aside>
</template>
