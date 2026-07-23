import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { vi } from "vitest";
import App from "./App";
import { api } from "./api/client";
import type { AppSettingsDto, PlanResponseDto, QueueItemDto, QueueSnapshotDto, QueueStreamMessage, SettingsDto } from "./api/generated";

const testHooks = vi.hoisted(() => ({
  queueMessageHandler: undefined as ((message: QueueStreamMessage) => void) | undefined,
}));

const testData = vi.hoisted(() => ({
  settings: {
    codec: "hevc",
    backend: "auto",
    decodeAcceleration: "software",
    parallelEnabled: false,
    parallelBackends: [],
    ratio: null,
    minVideoKbps: 250,
    maxVideoKbps: 0,
    container: "mp4",
    audioMode: "copy",
    audioBitrate: "128k",
    copySubtitles: true,
    copyExternalSubtitles: true,
    twoPass: false,
    encoderPreset: null,
    pixelFormat: "yuv420p",
    maxrateFactor: 1.25,
    bufsizeFactor: 4,
    overwrite: false,
    recursive: false,
  },
  appSettings: {
    language: "en",
    defaultPresetName: "default_hevc",
    keepPreviewTemp: true,
    recentPaths: [],
    lastSourcePath: "",
    lastOutputDir: "",
    workdirPath: "",
    ffmpegPath: "",
    ffprobePath: "",
    logLevel: "info",
    queueTableHeaderState: "",
  },
  queue: {
    state: { runState: "idle", activeRunId: null, items: [] },
    metrics: {
      totalItems: 0,
      queuedItems: 0,
      runningItems: 0,
      failedItems: 0,
      doneItems: 0,
      skippedItems: 0,
      cancelledItems: 0,
      readyItems: 0,
      completedItems: 0,
      totalDurationSec: 0,
      estimatedSavedBytes: null,
      queuePercent: 0,
      etaSec: null,
      currentItemId: null,
      currentFileName: null,
      currentFilePercent: null,
      currentSpeed: null,
    },
  },
})) as {
  settings: SettingsDto;
  appSettings: AppSettingsDto;
  queue: QueueSnapshotDto;
};

const planResponse = {
  items: [{
    sourcePath: "/videos/source.mp4",
    outputPath: "/videos/source_hevc.mp4",
    width: 1280,
    height: 720,
    duration: 2,
    sourceBitrate: 2_000_000,
    targetBitrate: 1_500_000,
    encoder: "libx265",
    backend: "cpu",
    warnings: [],
    skipReason: null,
  }],
  ffmpegPath: "/tools/ffmpeg",
  ffprobePath: "/tools/ffprobe",
  inputRoot: "/videos/source.mp4",
  outputRoot: "/videos",
};

const queueItem = (status: string, itemId: string): QueueItemDto => ({
  itemId,
  plan: planResponse.items[0],
  status,
  progress: { percent: status === "running" ? 35 : 0, speed: null, elapsedSec: null, currentPass: 0, totalPasses: 1 },
  error: status === "failed" || status === "cancelled" ? `${status} fixture` : null,
  result: null,
  runId: status === "running" ? "run-1" : null,
});

function queueSnapshot(items: QueueItemDto[], runState = "idle"): QueueSnapshotDto {
  return {
    state: { runState, activeRunId: runState === "idle" ? null : "run-1", items },
    metrics: {
      ...testData.queue.metrics,
      totalItems: items.length,
      queuedItems: items.filter((item) => item.status === "queued").length,
      runningItems: items.filter((item) => item.status === "running").length,
      failedItems: items.filter((item) => item.status === "failed").length,
      cancelledItems: items.filter((item) => item.status === "cancelled").length,
    },
  };
}

vi.mock("./api/client", () => ({
  api: {
    bootstrap: vi.fn().mockResolvedValue({ language: "en", defaultPresetName: "default_hevc", ffmpegPath: null, ffprobePath: null, settings: testData.settings, appSettings: testData.appSettings, queue: testData.queue }),
    listPresets: vi.fn().mockResolvedValue(["default_hevc"]),
    subscribeQueue: vi.fn().mockResolvedValue({ id: "q-1", unsubscribe: vi.fn().mockResolvedValue(undefined) }),
    subscribeActivity: vi.fn().mockResolvedValue({ id: "a-1", unsubscribe: vi.fn().mockResolvedValue(undefined) }),
    activityHistory: vi.fn().mockResolvedValue([]),
    subscriptionCount: vi.fn().mockResolvedValue(0),
    activityClear: vi.fn(),
    exportActivity: vi.fn(),
    redetectEncoders: vi.fn(),
    plan: vi.fn(),
    addToQueue: vi.fn(),
    startQueue: vi.fn(),
    pauseAfterCurrent: vi.fn(),
    stopQueue: vi.fn(),
    queueRetry: vi.fn(),
    removeQueue: vi.fn(),
    reorderQueue: vi.fn(),
    clearCompleted: vi.fn(),
    saveSettings: vi.fn(),
    saveAppSettings: vi.fn(),
    loadPreset: vi.fn().mockResolvedValue(testData.settings),
    savePreset: vi.fn(),
    deletePreset: vi.fn(),
    preview: vi.fn(),
    openAuxiliary: vi.fn(),
    closeAuxiliary: vi.fn(),
    pickFile: vi.fn(),
    pickDirectory: vi.fn(),
  },
}));

beforeEach(() => {
  vi.clearAllMocks();
  window.history.pushState({}, "", "/");
  vi.mocked(api.bootstrap).mockImplementation(async () => ({
    language: "en",
    defaultPresetName: "default_hevc",
    ffmpegPath: null,
    ffprobePath: null,
    settings: testData.settings,
    appSettings: testData.appSettings,
    queue: testData.queue,
  }));
  vi.mocked(api.listPresets).mockResolvedValue(["default_hevc"]);
  vi.mocked(api.subscribeQueue).mockResolvedValue({ id: "q-1", unsubscribe: vi.fn().mockResolvedValue(undefined) });
  vi.mocked(api.subscribeActivity).mockResolvedValue({ id: "a-1", unsubscribe: vi.fn().mockResolvedValue(undefined) });
  vi.mocked(api.activityHistory).mockResolvedValue([]);
  vi.mocked(api.plan).mockResolvedValue(planResponse);
  vi.mocked(api.addToQueue).mockResolvedValue(planResponse);
  vi.mocked(api.startQueue).mockResolvedValue(undefined);
  vi.mocked(api.pauseAfterCurrent).mockResolvedValue(undefined);
  vi.mocked(api.stopQueue).mockResolvedValue(undefined);
  vi.mocked(api.queueRetry).mockResolvedValue(undefined);
  vi.mocked(api.removeQueue).mockResolvedValue(undefined);
  vi.mocked(api.reorderQueue).mockResolvedValue(undefined);
  vi.mocked(api.clearCompleted).mockResolvedValue(undefined);
  vi.mocked(api.saveSettings).mockResolvedValue(undefined);
  vi.mocked(api.saveAppSettings).mockResolvedValue(undefined);
  vi.mocked(api.openAuxiliary).mockResolvedValue(undefined);
  testHooks.queueMessageHandler = undefined;
  vi.mocked(api.subscribeQueue).mockImplementation(async (handler) => {
    testHooks.queueMessageHandler = handler;
    return { id: "q-1", unsubscribe: vi.fn().mockResolvedValue(undefined) };
  });
});

afterEach(() => {
  window.history.pushState({}, "", "/");
});

test("renders the reference window regions and queue columns", async () => {
  render(<App />);
  expect(await screen.findByText("Source Setup")).toBeInTheDocument();
  expect(screen.getByText("Audio / Subtitles")).toBeInTheDocument();
  expect(screen.getByText("Estimated saved space")).toBeInTheDocument();
  expect(screen.getByText("Source bitrate")).toBeInTheDocument();
});

test("renders an operational activity auxiliary window", async () => {
  window.history.pushState({}, "", "/?window=activity");
  render(<App />);
  expect(await screen.findByRole("heading", { name: "Activity Log" })).toBeInTheDocument();
  expect(screen.getByText("No activity yet.")).toBeInTheDocument();
  window.history.pushState({}, "", "/");
});

test("does not plan when the source is empty", async () => {
  render(<App />);
  await screen.findByText("Source Setup");
  fireEvent.click(screen.getByRole("button", { name: /Plan/ }));
  expect(await screen.findByRole("alert")).toHaveTextContent("select a source");
  expect(api.plan).not.toHaveBeenCalled();
  expect(api.saveAppSettings).not.toHaveBeenCalled();
});

test("successful plan sends the request and persists deduplicated recent paths", async () => {
  const originalRecent = testData.appSettings.recentPaths;
  testData.appSettings.recentPaths = [
    "/videos/old-1.mp4", "/videos/source.mp4", "/videos/old-2.mp4", "/videos/old-3.mp4",
    "/videos/old-4.mp4", "/videos/old-5.mp4", "/videos/old-6.mp4", "/videos/old-7.mp4",
    "/videos/old-8.mp4", "/videos/old-9.mp4",
  ];
  try {
    render(<App />);
    const source = await screen.findByPlaceholderText("Select a source file or directory");
    fireEvent.change(source, { target: { value: "/videos/source.mp4" } });
    fireEvent.click(screen.getByRole("button", { name: /^◇ Plan$/ }));
    await screen.findByText(/Items: 1, ready: 1, skipped: 0/);
    expect(api.plan).toHaveBeenCalledWith(expect.objectContaining({
      inputPath: "/videos/source.mp4",
      outputDir: null,
      workdir: null,
      ffmpegPath: null,
      ffprobePath: null,
      settings: testData.settings,
    }));
    await waitFor(() => expect(api.saveAppSettings).toHaveBeenCalledWith(expect.objectContaining({
      lastSourcePath: "/videos/source.mp4",
      recentPaths: [
        "/videos/source.mp4", "/videos/old-1.mp4", "/videos/old-2.mp4", "/videos/old-3.mp4",
        "/videos/old-4.mp4", "/videos/old-5.mp4", "/videos/old-6.mp4", "/videos/old-7.mp4",
        "/videos/old-8.mp4", "/videos/old-9.mp4",
      ],
    })));
  } finally {
    testData.appSettings.recentPaths = originalRecent;
  }
});

test("successful add-to-queue persists app settings", async () => {
  render(<App />);
  fireEvent.change(await screen.findByPlaceholderText("Select a source file or directory"), {
    target: { value: "/videos/source.mp4" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Add to Queue/ }));
  await screen.findByText(/Items: 1, ready: 1, skipped: 0/);
  expect(api.addToQueue).toHaveBeenCalledWith(expect.objectContaining({
    inputPath: "/videos/source.mp4",
    outputDir: null,
    workdir: null,
    ffmpegPath: null,
    ffprobePath: null,
    settings: testData.settings,
  }));
  await waitFor(() => expect(api.saveAppSettings).toHaveBeenCalledWith(expect.objectContaining({
    lastSourcePath: "/videos/source.mp4",
    recentPaths: ["/videos/source.mp4"],
  })));
});

test("recent path save failure keeps the successful plan and shows a warning", async () => {
  vi.mocked(api.saveAppSettings).mockRejectedValueOnce(new Error("disk full"));
  render(<App />);
  fireEvent.change(await screen.findByPlaceholderText("Select a source file or directory"), {
    target: { value: "/videos/source.mp4" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Plan/ }));
  expect(await screen.findByText(/Items: 1, ready: 1, skipped: 0/)).toBeInTheDocument();
  expect(await screen.findByRole("alert")).toHaveTextContent("disk full");
});

test("plan passes configured tool paths and workdir", async () => {
  render(<App />);
  fireEvent.click(screen.getByRole("button", { name: "Advanced" }));
  fireEvent.change(screen.getByPlaceholderText("Select a source file or directory"), {
    target: { value: "/videos/source.mp4" },
  });
  fireEvent.change(screen.getByLabelText("Workdir"), { target: { value: "/work" } });
  fireEvent.change(screen.getByLabelText("FFmpeg path"), { target: { value: "/tools/ffmpeg" } });
  fireEvent.change(screen.getByLabelText("FFprobe path"), { target: { value: "/tools/ffprobe" } });
  fireEvent.click(screen.getByRole("button", { name: /^◇ Plan$/ }));
  await screen.findByText(/Items: 1, ready: 1, skipped: 0/);
  expect(api.plan).toHaveBeenCalledWith(expect.objectContaining({
    workdir: "/work",
    ffmpegPath: "/tools/ffmpeg",
    ffprobePath: "/tools/ffprobe",
  }));
});

test("plan errors are shown without persisting recent paths", async () => {
  vi.mocked(api.plan).mockRejectedValueOnce(new Error("probe failed"));
  render(<App />);
  fireEvent.change(await screen.findByPlaceholderText("Select a source file or directory"), {
    target: { value: "/videos/source.mp4" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Plan/ }));
  expect(await screen.findByRole("alert")).toHaveTextContent("probe failed");
  expect(api.saveAppSettings).not.toHaveBeenCalled();
});

test("queue controls call IPC with selected item IDs and respect run state", async () => {
  const originalQueue = testData.queue;
  testData.queue = queueSnapshot([
    queueItem("failed", "failed-1"),
    queueItem("cancelled", "cancelled-1"),
    queueItem("queued", "queued-1"),
  ]);
  try {
    render(<App />);
    await screen.findByText("Source Setup");
    const rows = screen.getAllByRole("row");
    fireEvent.click(within(rows[1]).getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(api.queueRetry).toHaveBeenCalledWith(["failed-1"]);
    fireEvent.click(screen.getByRole("button", { name: /Remove/ }));
    expect(api.removeQueue).toHaveBeenCalledWith(["failed-1"]);
    fireEvent.click(screen.getByRole("button", { name: /Start Queue/ }));
    await waitFor(() => expect(api.startQueue).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(screen.getByRole("button", { name: /Start Queue/ })).not.toBeDisabled());
  } finally {
    testData.queue = originalQueue;
  }
});

test("running queue exposes pause and stop actions", async () => {
  const originalQueue = testData.queue;
  testData.queue = queueSnapshot([queueItem("running", "running-1")], "running");
  try {
    render(<App />);
    await screen.findByText("Source Setup");
    fireEvent.click(screen.getByRole("button", { name: /Pause After Current/ }));
    fireEvent.click(screen.getByRole("button", { name: /Stop/ }));
    expect(api.pauseAfterCurrent).toHaveBeenCalledTimes(1);
    expect(api.stopQueue).toHaveBeenCalledTimes(1);
  } finally {
    testData.queue = originalQueue;
  }
});

test("busy state disables planning controls until the request completes", async () => {
  let resolvePlan!: (value: PlanResponseDto) => void;
  vi.mocked(api.plan).mockReturnValue(new Promise((resolve) => { resolvePlan = resolve; }));
  render(<App />);
  fireEvent.change(await screen.findByPlaceholderText("Select a source file or directory"), {
    target: { value: "/videos/source.mp4" },
  });
  fireEvent.click(screen.getByRole("button", { name: /Plan/ }));
  await waitFor(() => {
    expect(screen.getByRole("button", { name: /Plan/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Add to Queue/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: /^◇ Preview$/ })).toBeDisabled();
  });
  resolvePlan(planResponse);
  await waitFor(() => expect(screen.getByRole("button", { name: /Plan/ })).not.toBeDisabled());
});

test("queue stream snapshots update the main queue table", async () => {
  render(<App />);
  await screen.findByText("Source Setup");
  await waitFor(() => expect(testHooks.queueMessageHandler).toBeDefined());
  await act(async () => {
    testHooks.queueMessageHandler?.({ type: "snapshot", data: queueSnapshot([queueItem("running", "streamed-1")], "running") });
  });
  expect(await screen.findByText("running", { exact: true })).toBeInTheDocument();
});

test("queue auxiliary window retries only failed and cancelled items", async () => {
  const originalQueue = testData.queue;
  testData.queue = queueSnapshot([
    queueItem("failed", "failed-1"),
    queueItem("cancelled", "cancelled-1"),
    queueItem("queued", "queued-1"),
  ]);
  try {
    window.history.pushState({}, "", "/?window=queue");
    render(<App />);
    await screen.findByRole("heading", { name: "Queue" });
    const rows = await screen.findAllByRole("row");
    fireEvent.click(within(rows[1]).getByRole("checkbox"));
    fireEvent.click(within(rows[2]).getByRole("checkbox"));
    fireEvent.click(within(rows[3]).getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(api.queueRetry).toHaveBeenCalledWith(["failed-1", "cancelled-1"]));
  } finally {
    testData.queue = originalQueue;
  }
});

test("queue auxiliary window sends the requested reorder", async () => {
  const originalQueue = testData.queue;
  testData.queue = queueSnapshot([queueItem("queued", "queued-1"), queueItem("queued", "queued-2")]);
  try {
    window.history.pushState({}, "", "/?window=queue");
    render(<App />);
    await screen.findByRole("heading", { name: "Queue" });
    const rows = await screen.findAllByRole("row");
    fireEvent.click(within(rows[2]).getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "↑" }));
    await waitFor(() => expect(api.reorderQueue).toHaveBeenCalledWith(["queued-2", "queued-1"]));
  } finally {
    testData.queue = originalQueue;
  }
});

test("preview sends options and opens the auxiliary result window", async () => {
  vi.mocked(api.preview).mockResolvedValue({
    success: true,
    sourcePath: "/videos/source.mp4",
    sourceSamplePath: "/tmp/source-sample.mp4",
    encodedSamplePath: "/tmp/encoded-sample.mp4",
    sampleSourceSize: 100,
    sampleEncodedSize: 50,
    sampleCompressionRatio: 0.5,
    estimatedFullOutputSize: 500,
    notes: [],
    logPath: null,
    errorMessage: null,
  });
  render(<App />);
  fireEvent.change(await screen.findByPlaceholderText("Select a source file or directory"), {
    target: { value: "/videos/source.mp4" },
  });
  fireEvent.click(screen.getByRole("button", { name: /^◇ Preview$/ }));
  await screen.findByRole("status");
  expect(api.preview).toHaveBeenCalledWith(expect.objectContaining({ inputPath: "/videos/source.mp4", outputDir: null }), expect.objectContaining({ sampleMode: "middle" }));
  expect(api.openAuxiliary).toHaveBeenCalledWith("preview");
});

test("preview errors are shown without opening the result window", async () => {
  vi.mocked(api.preview).mockRejectedValueOnce(new Error("preview failed"));
  render(<App />);
  fireEvent.change(await screen.findByPlaceholderText("Select a source file or directory"), {
    target: { value: "/videos/source.mp4" },
  });
  fireEvent.click(screen.getByRole("button", { name: /^◇ Preview$/ }));
  expect(await screen.findByRole("alert")).toHaveTextContent("preview failed");
  expect(api.openAuxiliary).not.toHaveBeenCalled();
});

test("preset manager loads, saves, and deletes a preset", async () => {
  window.history.pushState({}, "", "/?window=presets");
  vi.mocked(api.listPresets).mockResolvedValue(["default_hevc", "portable"]);
  vi.mocked(api.loadPreset).mockResolvedValue({ ...testData.settings, ratio: 0.8 });
  vi.mocked(api.savePreset).mockResolvedValue("portable");
  vi.mocked(api.deletePreset).mockResolvedValue(undefined);
  render(<App />);
  expect(await screen.findByRole("heading", { name: "Preset Manager" })).toBeInTheDocument();
  const preset = screen.getByRole("combobox", { name: "Preset" });
  fireEvent.change(preset, { target: { value: "portable" } });
  await waitFor(() => expect(api.loadPreset).toHaveBeenCalledWith("portable"));
  fireEvent.change(screen.getByRole("textbox", { name: "Name" }), { target: { value: "portable" } });
  fireEvent.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(api.savePreset).toHaveBeenCalledWith("portable", expect.objectContaining({ ratio: 0.8 })));
  fireEvent.click(screen.getByRole("button", { name: "Delete" }));
  await waitFor(() => expect(api.deletePreset).toHaveBeenCalledWith("portable"));
});

test("settings window saves language and app paths", async () => {
  window.history.pushState({}, "", "/?window=settings");
  vi.mocked(api.bootstrap).mockResolvedValueOnce({
    language: "en",
    defaultPresetName: "default_hevc",
    ffmpegPath: null,
    ffprobePath: null,
    settings: testData.settings,
    appSettings: { ...testData.appSettings, workdirPath: "/bootstrap-work" },
    queue: testData.queue,
  });
  render(<App />);
  expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  await screen.findByDisplayValue("/bootstrap-work");
  fireEvent.change(screen.getByRole("combobox", { name: "Language" }), { target: { value: "zh_cn" } });
  fireEvent.change(screen.getByLabelText("Workdir"), { target: { value: "/settings-work" } });
  fireEvent.click(screen.getByRole("button", { name: "Save settings" }));
  await waitFor(() => expect(api.saveAppSettings).toHaveBeenCalledWith(expect.objectContaining({
    language: "zh_cn",
    workdirPath: "/settings-work",
  })));
});

test("activity panel never renders more than history limit", async () => {
  const many = Array.from({ length: 800 }, (_, index) => ({
    category: "process",
    message: `line-${index}`,
    timestamp: `t-${index}`,
  }));
  vi.mocked(api.activityHistory).mockResolvedValueOnce(many.slice(-500));
  window.history.pushState({}, "", "/?window=activity");
  render(<App />);
  await screen.findByRole("heading", { name: "Activity Log" });
  await waitFor(() => {
    const list = screen.getByTestId("activity-list");
    expect(Number(list.getAttribute("data-count"))).toBeLessThanOrEqual(500);
  });
});

test("queue subscription is cleaned up on unmount", async () => {
  const unsubscribe = vi.fn().mockResolvedValue(undefined);
  vi.mocked(api.subscribeQueue).mockImplementation(async (handler) => {
    testHooks.queueMessageHandler = handler;
    return { id: "q-cleanup", unsubscribe };
  });
  const view = render(<App />);
  await screen.findByText("Source Setup");
  await waitFor(() => expect(api.subscribeQueue).toHaveBeenCalled());
  view.unmount();
  await waitFor(() => expect(unsubscribe).toHaveBeenCalled());
});
