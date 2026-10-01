<script setup lang="ts">
import { computed } from "vue";
import type { Translator } from "../core/i18n";
import type { TimelineData, TimelineDays } from "../core/types";
const props = defineProps<{
  data: TimelineData | null;
  loading: boolean;
  error: boolean;
  days: TimelineDays;
  selectedDate: string;
  noSources: boolean;
  t: Translator;
}>();
const emit = defineEmits<{ days: [value: TimelineDays]; select: [date: string]; retry: [] }>();
const maximum = computed(() => Math.max(1, ...props.data?.buckets.map((bucket) => bucket.count) ?? []));
</script>
<template>
  <section class="timeline-panel" :aria-label="t('timeline')" :aria-busy="loading">
    <div class="timeline-heading">
      <div><h2>{{ t("dailyActivity") }}</h2><p>{{ t("timelineScope") }}</p></div>
      <fieldset class="timeline-ranges">
        <legend class="sr-only">{{ t("timelineRange") }}</legend>
        <button v-for="range in ([7, 30, 90] as const)" :key="range" type="button" :aria-pressed="range === days"
          @click="emit('days', range)">{{ t("lastDays", { count: range }) }}</button>
      </fieldset>
    </div>
    <p class="timeline-timezone">{{ t("timelineTimezone") }}</p>
    <p v-if="noSources" class="notice">{{ t("noSourcesHint") }}</p>
    <div v-if="error" class="notice error" role="alert">
      {{ t("timelineError") }} <button type="button" :disabled="loading" @click="emit('retry')">{{ t("retry") }}</button>
    </div>
    <p v-if="loading" class="timeline-status" role="status">{{ t("timelineLoading") }}</p>
    <template v-else-if="data && !error">
      <div class="timeline-total" role="status">{{ t("timelineTotal", { count: data.total, days: data.days }) }}</div>
      <p v-if="data.total === 0 && !noSources" class="timeline-status">{{ t("timelineEmpty") }}</p>
      <svg class="timeline-chart" :viewBox="`0 0 ${data.days * 10} 100`" preserveAspectRatio="none" aria-hidden="true" focusable="false">
        <line x1="0" y1="99" :x2="data.days * 10" y2="99" />
        <rect v-for="(bucket, index) in data.buckets" :key="bucket.date" :x="index * 10 + 1"
          :y="99 - bucket.count / maximum * 94" width="8" :height="bucket.count / maximum * 94"
          :class="{ selected: bucket.date === selectedDate }" />
      </svg>
      <p class="timeline-chart-caption"><span>{{ data.buckets[0]!.date }}</span><span>{{ t("chartMaximum", { count: maximum === 1 && data.total === 0 ? 0 : maximum }) }}</span><span>{{ data.buckets.at(-1)!.date }}</span></p>
      <p id="timeline-day-help" class="timeline-help">{{ t("timelineHelp") }}</p>
      <ol class="timeline-days" aria-describedby="timeline-day-help">
        <li v-for="bucket in data.buckets" :key="bucket.date">
          <button type="button" :aria-pressed="bucket.date === selectedDate" :aria-label="t('dayCount', { date: bucket.date, count: bucket.count })"
            @click="emit('select', bucket.date)">
            <time :datetime="bucket.date">{{ bucket.date.slice(5) }}</time><strong>{{ bucket.count }}</strong>
          </button>
        </li>
      </ol>
    </template>
  </section>
</template>
