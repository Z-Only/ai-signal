import { readFileSync } from "node:fs";
import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import App from "../src/App.vue";
import { news, respond } from "./fixtures";

const cssPath = "../src/style.css";
const css = readFileSync(new URL(cssPath, import.meta.url), "utf8");
const styles: HTMLStyleElement[] = [];

// jsdom has no layout engine or media-query evaluation. Apply the stylesheet's
// matching width rules to guard the mobile layout contract, not pixel geometry.
function applyWidthStyles(width: number) {
  const source = document.createElement("style");
  source.textContent = css;
  document.head.append(source);
  const rules = Array.from(source.sheet!.cssRules).flatMap((rule) => {
    if (rule instanceof CSSStyleRule) return [rule.cssText];
    if (!(rule instanceof CSSMediaRule)) return [];
    const match = /^\((max|min)-width: (\d+)px\)$/.exec(rule.conditionText);
    if (!match || (match[1] === "max" ? width > +match[2]! : width < +match[2]!)) return [];
    return Array.from(rule.cssRules, (nested) => nested.cssText);
  });
  source.remove();
  const style = document.createElement("style");
  style.textContent = rules.join("\n");
  document.head.append(style);
  styles.push(style);
}

afterEach(() => styles.splice(0).forEach((style) => style.remove()));

describe("page header responsive layout contract", () => {
  it.each([320, 390, 700])("keeps refresh in its own grid cell at %ipx", async (width) => {
    applyWidthStyles(width);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(news())));
    const wrapper = mount(App, { attachTo: document.body });
    await flushPromises();

    for (const locale of ["zh-CN", "en"]) {
      await wrapper.findAll("select")[0]!.setValue(locale);
      for (const theme of ["light", "dark"]) {
        await wrapper.findAll("select")[1]!.setValue(theme);
        for (const view of [0, 1, 2]) {
          await wrapper.findAll("nav button")[view]!.trigger("click");
          await flushPromises();
          const computed = (selector: string) => getComputedStyle(wrapper.get(selector).element);
          expect(computed(".page-head").display).toBe("grid");
          expect(computed(".page-head").gridTemplateColumns).toBe("minmax(0, 1fr) auto");
          expect(computed(".page-head > div").display).toBe("contents");
          expect(computed(".page-head .reload").position).not.toMatch(/absolute|fixed/);
          expect(computed(".page-head .reload").gridColumn).toBe("2");
          expect(computed(".page-head .reload").gridRow).toBe("1");
          expect(computed(".page-head h1").gridColumn).toBe("1 / -1");
          expect(computed(".page-head p").gridColumn).toBe("1 / -1");
          expect(wrapper.get(".page-head .reload").attributes("aria-label")).toBeTruthy();
        }
      }
    }
  });

  it.each([701, 1440])("preserves the wider side-by-side header at %ipx", async (width) => {
    applyWidthStyles(width);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(respond(news())));
    const wrapper = mount(App, { attachTo: document.body });
    await flushPromises();
    expect(getComputedStyle(wrapper.get(".page-head").element).display).toBe("flex");
    expect(getComputedStyle(wrapper.get(".page-head > div").element).display).toBe("block");
    expect(getComputedStyle(wrapper.get(".page-head .reload").element).position).not.toMatch(/absolute|fixed/);
  });
});
