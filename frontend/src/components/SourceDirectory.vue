<script setup lang="ts">
import type { Translator } from "../core/i18n";
import { formatDate, safeUrl, sourceInitial } from "../core/news";
import type { Locale, NewsData } from "../core/types";
const props = defineProps<{
  data: NewsData;
  locale: Locale;
  t: Translator;
  compact?: boolean;
}>();
const emit = defineEmits<{ expand: [] }>();
const status = (id: string) =>
  props.data.run?.details.find((detail) => detail.source === id);
</script>
<template>
  <div v-if="compact" class="sources-card">
    <div class="section-head">
      <h2>{{ t("radar") }}</h2>
      <button type="button" @click="emit('expand')">{{ t("viewAll") }}</button>
    </div>
    <div v-for="source in data.sources" :key="source.id" class="source-row">
      <span class="source-icon" :data-source="source.id" aria-hidden="true">{{
        sourceInitial(source.id)
      }}</span>
      <div>
        <b>{{ source.name }}</b
        ><small>{{
          status(source.id)?.status === "ok"
            ? t("syncedRss")
            : status(source.id)?.status === "error"
              ? t("failedRss")
              : t("rss")
        }}</small>
      </div>
      <span
        :class="[
          'health',
          {
            bad: status(source.id)?.status === 'error',
            pending: !status(source.id),
          },
        ]"
        aria-hidden="true"
      ></span>
    </div>
  </div>
  <section v-else class="sources-page" :aria-label="t('sources')">
    <article v-for="source in data.sources" :key="source.id">
      <div class="source-icon big" :data-source="source.id" aria-hidden="true">
        {{ sourceInitial(source.id) }}
      </div>
      <h2>{{ source.name }}</h2>
      <p>
        {{
          status(source.id)?.status === "ok"
            ? t("syncedCount", { count: status(source.id)?.items ?? 0 })
            : status(source.id)?.status === "error"
              ? t("sourceFailed")
              : t("waiting")
        }}
      </p>
      <small v-if="status(source.id)">{{
        t("checked", {
          date: formatDate(status(source.id)!.checked_at, locale),
        })
      }}</small
      ><a :href="safeUrl(source.home)" target="_blank" rel="noopener noreferrer"
        >{{ t("visitSource")
        }}<span class="sr-only"> ({{ t("newTab") }})</span></a
      >
    </article>
  </section>
</template>
