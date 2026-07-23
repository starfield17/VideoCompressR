import { expect, test } from "@playwright/test";

test("browser mode renders a stable main-window screenshot", async ({ page }, testInfo) => {
  await page.goto("/");
  await expect(page.locator(".toolbar")).toBeVisible();
  await expect(page.locator(".source-card")).toBeVisible();
  await expect(page.locator(".tabs-card")).toBeVisible();
  await expect(page.locator(".jobs-card")).toBeVisible();
  await expect(page.locator(".statusbar")).toBeVisible();
  const screenshot = await page.screenshot({ fullPage: true });
  await testInfo.attach("main-window.png", { body: screenshot, contentType: "image/png" });
  expect(screenshot.byteLength).toBeGreaterThan(1_000);
});

test("browser mode validates source selection before invoking the runtime", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: /Plan/ }).click();
  await expect(page.getByRole("alert")).toContainText("select a source");
});
