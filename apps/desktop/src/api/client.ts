import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  ActivityEventDto,
  AppSettingsDto,
  BootstrapDto,
  PlanRequestDto,
  PlanResponseDto,
  PreviewOptionsDto,
  PreviewResultDto,
  QueueStreamMessage,
  SettingsDto,
} from "./generated";
import { queueChannel } from "./channels";

export type SubscriptionHandle = {
  id: string;
  unsubscribe(): Promise<void>;
};

const DEFAULT_ACTIVITY_HISTORY_LIMIT = 500;

export const api = {
  bootstrap: () => invoke<BootstrapDto>("bootstrap"),
  openAuxiliary: (kind: string) => invoke<void>("open_aux_window", { kind }),
  closeAuxiliary: () => getCurrentWindow().close(),
  plan: (request: PlanRequestDto) => invoke<PlanResponseDto>("plan_encode", { request }),
  addToQueue: (request: PlanRequestDto) => invoke<PlanResponseDto>("queue_add", { request }),
  startQueue: () => invoke<void>("queue_start"),
  pauseAfterCurrent: () => invoke<void>("queue_pause_after_current"),
  stopQueue: () => invoke<void>("queue_stop"),
  queueRetry: (itemIds: string[]) => invoke<void>("queue_retry", { itemIds }),
  removeQueue: (itemIds: string[]) => invoke<void>("queue_remove", { itemIds }),
  reorderQueue: (orderedIds: string[]) => invoke<void>("queue_reorder", { orderedIds }),
  clearCompleted: () => invoke<void>("queue_clear_completed"),
  saveSettings: (settings: SettingsDto) => invoke<void>("save_settings", { settings }),
  saveAppSettings: (settings: AppSettingsDto) => invoke<void>("save_app_settings", { settings }),
  listPresets: () => invoke<string[]>("preset_list"),
  loadPreset: (name: string) => invoke<SettingsDto>("preset_load", { name }),
  savePreset: (name: string, settings: SettingsDto) => invoke<string>("preset_save", { name, settings }),
  deletePreset: (name: string) => invoke<void>("preset_delete", { name }),
  preview: (request: PlanRequestDto, options: PreviewOptionsDto) => invoke<PreviewResultDto>("preview", { request, options }),
  subscribeQueue: async (onMessage: (message: QueueStreamMessage) => void): Promise<SubscriptionHandle> => {
    const channel = queueChannel(onMessage);
    const id = await invoke<string>("queue_subscribe", { channel });
    return {
      id,
      unsubscribe: () => invoke<void>("queue_unsubscribe", { subscriptionId: id }),
    };
  },
  subscribeActivity: async (onMessage: (message: QueueStreamMessage) => void): Promise<SubscriptionHandle> => {
    const channel = queueChannel(onMessage);
    const id = await invoke<string>("activity_subscribe", { channel });
    return {
      id,
      unsubscribe: () => invoke<void>("activity_unsubscribe", { subscriptionId: id }),
    };
  },
  activityHistory: (limit = DEFAULT_ACTIVITY_HISTORY_LIMIT) =>
    invoke<ActivityEventDto[]>("activity_history", { limit }),
  activityClear: () => invoke<void>("activity_clear"),
  exportActivity: async () => {
    const path = await save({
      defaultPath: "video-compressor.log",
      filters: [{ name: "Log files", extensions: ["log", "txt"] }],
    });
    if (path) await invoke<void>("activity_export", { path });
  },
  redetectEncoders: () => invoke<void>("redetect_encoders"),
  subscriptionCount: () => invoke<number>("subscription_count"),
  pickFile: () => open({ multiple: false, directory: false }),
  pickDirectory: () => open({ multiple: false, directory: true }),
};
