<script setup lang="ts">
import { computed } from "vue";
import { categoryLabel, type Translator } from "../core/i18n";
import { formatDate, safeUrl, sourceInitial } from "../core/news";
import type { Article, Locale, Source } from "../core/types";
const props = defineProps<{
  articles: Article[];
  sources: Source[];
  locale: Locale;
  t: Translator;
}>();
const lead = computed(() => props.articles[0]);
const others = computed(() => props.articles.slice(1));
const sourceName = (id: string) =>
  props.sources.find((source) => source.id === id)?.name ?? id;
</script>
<template>
  <section>
    <div class="section-head">
      <h2>{{ t("latest") }}</h2>
      <span>{{ t("order") }}</span>
    </div>
    <a
      v-if="lead"
      :href="safeUrl(lead.url)"
      target="_blank"
      rel="noopener noreferrer"
      class="lead-card"
    >
      <div class="lead-top">
        <span class="tag">{{ categoryLabel(locale, lead.category) }}</span
        ><span>{{ t("latestRelease") }}</span>
      </div>
      <h2>{{ lead.title }}</h2>
      <p>{{ lead.summary || t("missingSummary") }}</p>
      <div class="article-foot">
        <span
          class="source-icon"
          :data-source="lead.source"
          aria-hidden="true"
          >{{ sourceInitial(lead.source) }}</span
        ><b>{{ sourceName(lead.source) }}</b
        ><time :datetime="lead.published_at">{{
          formatDate(lead.published_at, locale)
        }}</time
        ><span class="read"
          >{{ t("read")
          }}<span class="sr-only"> ({{ t("newTab") }})</span></span
        >
      </div>
      <div class="orb" aria-hidden="true"></div>
    </a>
    <div class="section-head stream-head">
      <h2>{{ t("stream") }} <span>/ LATEST</span></h2>
      <span>{{ t("updateCount", { count: others.length }) }}</span>
    </div>
    <a
      v-for="(article, index) in others"
      :key="article.id"
      :href="safeUrl(article.url)"
      target="_blank"
      rel="noopener noreferrer"
      class="news-card"
    >
      <div class="news-index" aria-hidden="true">
        {{ String(index + 2).padStart(2, "0") }}
      </div>
      <div class="news-body">
        <div class="news-meta">
          <span
            class="source-icon small"
            :data-source="article.source"
            aria-hidden="true"
            >{{ sourceInitial(article.source) }}</span
          ><b>{{ sourceName(article.source) }}</b
          ><span class="sep" aria-hidden="true">·</span
          ><time :datetime="article.published_at">{{
            formatDate(article.published_at, locale)
          }}</time
          ><span class="category">{{
            categoryLabel(locale, article.category)
          }}</span>
        </div>
        <h3>{{ article.title }}</h3>
        <p v-if="article.summary">{{ article.summary }}</p>
        <span v-else class="no-summary">{{ t("noSummary") }}</span>
      </div>
      <span class="external" aria-hidden="true">↗</span
      ><span class="sr-only">{{ t("newTab") }}</span>
    </a>
  </section>
</template>
