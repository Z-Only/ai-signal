import { validDate } from "./filters";
import type { TimelineData, TimelineDays } from "./types";

export function parseTimeline(value: unknown, days: TimelineDays): TimelineData {
  if (!value || typeof value !== "object") throw new Error("Invalid timeline");
  const data = value as TimelineData;
  if (data.timezone !== "UTC" || data.days !== days || !Array.isArray(data.buckets) ||
      data.buckets.length !== days || !Number.isSafeInteger(data.total) || data.total < 0)
    throw new Error("Invalid timeline");
  let total = 0;
  let previous: number | undefined;
  for (const bucket of data.buckets) {
    if (!bucket || typeof bucket.date !== "string" || !validDate(bucket.date) ||
        !Number.isSafeInteger(bucket.count) || bucket.count < 0)
      throw new Error("Invalid timeline bucket");
    const time = Date.parse(`${bucket.date}T00:00:00Z`);
    if (previous !== undefined && time - previous !== 86_400_000) throw new Error("Invalid timeline order");
    previous = time;
    total += bucket.count;
  }
  if (total !== data.total) throw new Error("Invalid timeline total");
  return data;
}
