import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
const cssPath = "../src/style.css";
const css = readFileSync(new URL(cssPath, import.meta.url), "utf8");

let style: HTMLStyleElement;
beforeEach(() => {
  style = document.createElement("style");
  style.textContent = css;
  document.head.append(style);
  document.body.innerHTML = `
    <div class="sources-page"><article><p>35 articles synced</p><small>Checked yesterday</small></article></div>
    <a class="lead-card"><div class="article-foot"><time>10/01 05:00</time></div><p>Source summary</p></a>
    <div class="news-meta"><span class="category">Models</span></div>
    <input class="search-input"><p class="search-help">Search all collected articles</p>
  `;
});
afterEach(() => style.remove());

function computed(selector: string) {
  return getComputedStyle(document.querySelector(selector)!);
}
function luminance(color: string) {
  const channels = color.match(/[\d.]+/g)!.slice(0, 3).map(Number).map((value) => {
    const scaled = value / 255;
    return scaled <= 0.04045 ? scaled / 12.92 : ((scaled + 0.055) / 1.055) ** 2.4;
  });
  return channels.reduce((total, value, index) => total + value * [0.2126, 0.7152, 0.0722][index]!, 0);
}
function contrast(foreground: string, background: string) {
  const [low, high] = [luminance(foreground), luminance(background)].sort((a, b) => a - b);
  return (high! + 0.05) / (low! + 0.05);
}

describe("reading contrast", () => {
  it.each([
    [".sources-page p", ".sources-page article"],
    [".sources-page small", ".sources-page article"],
    [".article-foot time", ".lead-card"],
    [".lead-card p", ".lead-card"],
    [".news-meta .category", ".news-meta .category"],
    [".search-help", ":root"],
  ])("keeps %s above normal-text contrast in both themes", (text, background) => {
    for (const theme of ["light", "dark"]) {
      document.documentElement.dataset.theme = theme;
      expect(contrast(computed(text).color, computed(background).backgroundColor)).toBeGreaterThanOrEqual(4.5);
    }
  });
});
