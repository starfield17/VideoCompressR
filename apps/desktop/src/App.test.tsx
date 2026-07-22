import { render, screen } from "@testing-library/react";
import { vi } from "vitest";
import App from "./App";

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
    state: { runState: "idle", items: [] },
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
}));

vi.mock("./api/client", () => ({
  api: {
    bootstrap: vi.fn().mockResolvedValue({ language: "en", defaultPresetName: "default_hevc", ffmpegPath: null, ffprobePath: null, settings: testData.settings, appSettings: testData.appSettings, queue: testData.queue }),
    listPresets: vi.fn().mockResolvedValue(["default_hevc"]),
    subscribeQueue: vi.fn().mockResolvedValue(undefined),
    subscribeActivity: vi.fn().mockResolvedValue(undefined),
    activityHistory: vi.fn().mockResolvedValue([]),
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
