<script setup lang="ts">
import { computed } from "vue";
import { scheduleLabel, type Translator } from "../core/i18n";
import { formatDate, recentCount } from "../core/news";
import type { Locale, NewsData } from "../core/types";
const props = defineProps<{ data: NewsData; locale: Locale; t: Translator }>();
const total = computed(
  () =>
    props.data.stats?.total_articles ??
    props.data.pagination?.total ??
    props.data.articles.length,
);
const recent = computed(
  () => props.data.stats?.recent_articles ?? recentCount(props.data.articles),
);
const sourceCount = computed(
  () => props.data.stats?.total_sources ?? props.data.sources.length,
);
</script>
<template>
  <div class="stats">
    <div>
      <small>{{ t("collected") }}</small
      ><strong
        >{{ String(total).padStart(2, "0")
        }}<span>{{ t("articlesUnit") }}</span></strong
      >
    </div>
    <div>
      <small>{{ t("recent") }}</small
      ><strong
        >{{ String(recent).padStart(2, "0")
        }}<span>{{ t("updatesUnit") }}</span></strong
      >
    </div>
    <div>
      <small>{{ t("officialSources") }}</small
      ><strong
        >{{ String(sourceCount).padStart(2, "0")
        }}<span>{{ t("channels") }}</span></strong
      >
    </div>
    <div class="sync-stat">
      <small>{{ t("sync") }}</small
      ><b
        ><span class="pulse" aria-hidden="true"></span
        >{{ scheduleLabel(locale, data.schedule) }}</b
      ><span>{{
        data.run?.finished_at
          ? t("lastSync", { date: formatDate(data.run.finished_at, locale) })
          : t("waiting")
      }}</span>
    </div>
  </div>
</template>
