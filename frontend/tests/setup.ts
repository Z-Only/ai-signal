import { afterEach, beforeEach, vi } from "vitest";
import { enableAutoUnmount } from "@vue/test-utils";
enableAutoUnmount(afterEach);
beforeEach(() => {
  localStorage.clear();
  window.history.replaceState(null, "", "/");
  document.documentElement.removeAttribute("data-theme");
  const media = new EventTarget();
  Object.assign(media, {
    matches: false,
    media: "(prefers-color-scheme: dark)",
  });
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => media),
  );
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  document.body.innerHTML = "";
});
