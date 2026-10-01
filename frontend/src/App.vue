<script setup lang="ts">
import { computed, ref } from "vue";
import ArticleFeed from "./components/ArticleFeed.vue";
import CategoryFilters from "./components/CategoryFilters.vue";
import NewsStats from "./components/NewsStats.vue";
import PreferencesControl from "./components/PreferencesControl.vue";
import SidebarNav from "./components/SidebarNav.vue";
import SourceDirectory from "./components/SourceDirectory.vue";
import { useNews } from "./composables/useNews";
import { usePreferences } from "./composables/usePreferences";
import type { Category, View } from "./core/types";
const { data, loading, error, hasMore, load } = useNews();
const { locale, theme, t } = usePreferences();
const active = ref<Category>("全部资讯");
const view = ref<View>("feed");
const articles = computed(() =>
  data.value.articles.filter(
    (article) =>
      active.value === "全部资讯" || article.category === active.value,
  ),
);
const sourceCount = computed(
  () => data.value.stats?.total_sources ?? data.value.sources.length,
);
const total = computed(
  () =>
    data.value.stats?.total_articles ??
    data.value.pagination?.total ??
    data.value.articles.length,
);
const fullDate = computed(() =>
  new Date().toLocaleDateString(locale.value, {
    year: "numeric",
    month: "long",
    day: "numeric",
    weekday: "long",
  }),
);
</script>
<template>
  <a class="skip-link" href="#main-content">{{ t("skip") }}</a>
  <div class="shell">
    <SidebarNav
      :view="view"
      :source-count="sourceCount"
      :t="t"
      @navigate="view = $event"
    />
    <div class="workspace">
      <header>
        <div class="breadcrumb">
          {{ t("workspace") }} <span>/</span> {{ t(view) }}
        </div>
        <div class="header-right">
          <PreferencesControl
            :locale="locale"
            :theme="theme"
            :t="t"
            @locale="locale = $event"
            @theme="theme = $event"
          /><span class="private">{{ t("personal") }}</span
          ><span class="avatar" aria-hidden="true">AI</span>
        </div>
      </header>
      <main id="main-content" tabindex="-1">
        <div class="page-head">
          <div>
            <div class="eyebrow">{{ fullDate }}</div>
            <h1>{{ t(view === "feed" ? "feedTitle" : "sourcesTitle") }}</h1>
            <p>
              {{
                t(view === "feed" ? "feedDescription" : "sourcesDescription")
              }}
            </p>
          </div>
          <button
            class="reload"
            type="button"
            :aria-label="loading ? t('loading') : t('refresh')"
            :disabled="loading"
            @click="load()"
          >
            <span :class="{ spin: loading }" aria-hidden="true">⟳</span
            >{{ loading ? t("loading") : t("refresh") }}
          </button>
        </div>
        <NewsStats :data="data" :locale="locale" :t="t" />
        <div v-if="error" class="notice error" role="alert">
          {{ t("error") }}
          <button type="button" :disabled="loading" @click="load()">
            {{ t("retry") }}
          </button>
        </div>
        <div
          v-if="data.run?.status === 'partial' || data.run?.status === 'failed'"
          class="notice"
          role="status"
        >
          {{ t(data.run.status === "partial" ? "partial" : "failed") }}
        </div>
        <template v-if="view === 'feed'">
          <CategoryFilters
            :active="active"
            :count="total"
            :locale="locale"
            :t="t"
            @select="active = $event"
          />
          <div class="feed-results" :aria-busy="loading">
            <div
              v-if="loading && !data.articles.length"
              class="empty"
              role="status"
            >
              {{ t("connectingLibrary") }}
            </div>
            <div v-else-if="!articles.length" class="empty" role="status">
              <span aria-hidden="true">◎</span>
              <h2>{{ t("emptyTitle") }}</h2>
              <p>
                {{ t(data.articles.length ? "emptyCategory" : "emptyLibrary") }}
              </p>
            </div>
            <div v-else class="content-grid">
              <ArticleFeed
                :articles="articles"
                :sources="data.sources"
                :locale="locale"
                :t="t"
              />
              <aside class="right-column">
                <div class="editor-note">
                  <div class="mini-label">SIGNAL, NOT NOISE</div>
                  <h2>
                    {{ t("attention") }}<br />{{ t("progress")
                    }}<span aria-hidden="true">✳</span>
                  </h2>
                  <p>{{ t("editorBody") }}</p>
                  <div class="note-bottom">{{ t("editorFoot") }}</div>
                </div>
                <SourceDirectory
                  :data="data"
                  :locale="locale"
                  :t="t"
                  compact
                  @expand="view = 'sources'"
                />
                <div class="method-note">
                  <h3>{{ t("methodTitle") }}</h3>
                  <p>{{ t("methodBody") }}</p>
                  <p>{{ t("timezone") }}</p>
                </div>
              </aside>
            </div>
          </div>
          <div v-if="hasMore" class="pagination">
            <p>{{ t("loaded", { count: data.articles.length }) }}</p>
            <button
              class="reload"
              type="button"
              :disabled="loading"
              @click="load(true)"
            >
              {{ loading ? t("loading") : t("loadMore") }}
            </button>
          </div>
        </template>
        <SourceDirectory v-else :data="data" :locale="locale" :t="t" />
        <footer>
          <span>{{ t("brand") }} <b>·</b> THE AI SIGNAL</span
          ><span>{{ t("copyright") }}</span>
        </footer>
      </main>
    </div>
  </div>
</template>
