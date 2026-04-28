import { test, expect } from "@playwright/test";

test("loads shell and shows top-bar title", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("wanlogger")).toBeVisible();
});

test("language toggle switches between ja and en", async ({ page }) => {
  await page.goto("/");
  const toggle = page.getByRole("button", { name: /JA|EN/ });
  const before = await toggle.textContent();
  await toggle.click();
  const after = await toggle.textContent();
  expect(after).not.toEqual(before);
});
