describe("desktop smoke", () => {
  it("renders the legacy-compatible main regions", async () => {
    await expect($(".toolbar")).toBeDisplayed();
    await expect($(".source-card")).toBeDisplayed();
    await expect($(".tabs-card")).toBeDisplayed();
    await expect($(".jobs-card")).toBeDisplayed();
    await expect($(".statusbar")).toBeDisplayed();
    await expect($("th=Name")).toBeDisplayed();
    await expect($("th=Progress")).toBeDisplayed();
  });
});
