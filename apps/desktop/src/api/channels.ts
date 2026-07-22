import { Channel } from "@tauri-apps/api/core";
import type { QueueStreamMessage } from "./generated";

export function queueChannel(onMessage: (message: QueueStreamMessage) => void): Channel<QueueStreamMessage> {
  const channel = new Channel<QueueStreamMessage>();
  channel.onmessage = onMessage;
  return channel;
}
