# UX contract

The reference Qt window is the behavioral source. The Tauri desktop keeps the
same functional regions and order:

1. toolbar actions for Add Files, Add Folder, Add to Queue, Start Queue, Pause
   After Current, Preview, Stop, Queue, Activity Log, Presets, and Settings;
2. Source Setup with source, output, and preset controls;
3. Basic, Video, Audio/Subtitles, Preview, and Advanced tabs;
4. queue summary, progress, table, and status bar.

The queue table retains Name, Folder, Resolution, Duration, Source bitrate,
Target bitrate, Encoder, Output, Tags, Status, and Progress. Rust snapshots
own status, progress, retry, remove, reorder, and cancellation semantics.

Auxiliary windows are Queue, Activity Log, Preset Manager, Settings, and
Preview Result. They are created as separate Tauri webview windows and reuse
the generated IPC client. Minimum main window size is 760×520, with a
scrollable center area for small screens. English and Simplified Chinese use
the copied legacy key set. Pixel-level Qt font and title-bar differences are
accepted by the architecture definition.
