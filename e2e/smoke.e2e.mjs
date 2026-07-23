describe("desktop smoke", () => {
  it("runs the packaged app through planning, queue execution, and auxiliary windows", async () => {
    await expect($(".toolbar")).toBeDisplayed();
    await expect($(".source-card")).toBeDisplayed();
    await expect($(".tabs-card")).toBeDisplayed();
    await expect($(".jobs-card")).toBeDisplayed();
    await expect($(".statusbar")).toBeDisplayed();
    await expect($("th=Name")).toBeDisplayed();
    await expect($("th=Progress")).toBeDisplayed();

    const sourcePath = process.env.E2E_SOURCE_PATH;
    if (!sourcePath) throw new Error("E2E_SOURCE_PATH is required");
    const source = $('input[placeholder="Select a source file or directory"]');
    await source.setValue(sourcePath);
    await $("button=◇ Plan").click();
    await expect($(".plan-note")).toHaveTextContaining("Items: 1");

    await $("button=▤ Add to Queue").click();
    await browser.waitUntil(async () => (await $("tbody tr").isDisplayed()), {
      timeout: 30_000,
      timeoutMsg: "queue item did not appear",
    });
    const start = $("button=▶ Start Queue");
    await browser.waitUntil(async () => (await start.isEnabled()), {
      timeout: 30_000,
      timeoutMsg: "start button did not become enabled",
    });
    await start.click();
    await browser.waitUntil(async () => (await $("td=running").isExisting()), {
      timeout: 30_000,
      timeoutMsg: "queue item did not enter running state",
    });
    await expect($(".statusbar")).toHaveTextContaining("running");

    await $("button=■ Stop").click();
    await browser.waitUntil(async () => (await $("td=cancelled").isExisting()), {
      timeout: 30_000,
      timeoutMsg: "queue item did not become cancelled after stop",
    });
    await browser.waitUntil(async () => (await $(".statusbar").getText()).includes("Stage: -"), {
      timeout: 30_000,
      timeoutMsg: "queue did not return to idle after stop",
    });

    const mainHandle = await browser.getWindowHandle();
    for (const [button, heading] of [
      ["≡ Activity Log", "Activity Log"],
      ["▣ Presets", "Preset Manager"],
      ["⚙ Settings", "Settings"],
    ]) {
      await $(`button=${button}`).click();
      await browser.waitUntil(async () => (await browser.getWindowHandles()).length > 1, {
        timeout: 30_000,
        timeoutMsg: `${heading} window did not open`,
      });
      const handles = await browser.getWindowHandles();
      await browser.switchToWindow(handles.find((handle) => handle !== mainHandle));
      await expect($(`h1=${heading}`)).toBeDisplayed();
      await $("button=×").click();
      await browser.switchToWindow(mainHandle);
      await browser.waitUntil(async () => (await browser.getWindowHandles()).length === 1, {
        timeout: 30_000,
        timeoutMsg: `${heading} window did not close`,
      });
    }
  });
});
