# In-App Console Log Viewer Plan

## 1. `src-tauri/Cargo.toml`
Add `registry` feature to tracing-subscriber:
```
tracing-subscriber = { version = "0.3", features = ["env-filter", "registry"] }
```

## 2. `src-tauri/src/log_console.rs` (new file)
- LogEntry struct (id, timestamp, level, target, message)
- ConsoleLogLayer — custom tracing_subscriber::Layer
- ConsoleLogState — Tauri managed state (Arc<RwLock<VecDeque<LogEntry>>>, AtomicU64 counter)
- console_logs_get(since_id: u64) command
- console_logs_clear() command

## 3. `src-tauri/src/commands/mod.rs`
Add: `pub mod log_console;`

## 4. `src-tauri/src/lib.rs`
- Register ConsoleLogState as managed state
- Register console_logs_get / console_logs_clear
- Modify init_logging() to add ConsoleLogLayer alongside file writer

## 5. `ui/src/components/LogConsole.vue` (new file)
- Bottom panel with resizable height
- Filter dropdown (ALL/TRACE/DEBUG/INFO/WARN/ERROR)
- Color-coded log entries, auto-scroll, clear button
- Polling setInterval(200ms)

## 6. `ui/src/App.vue`
- Import and render LogConsole
- Add Console sidebar button
- Add Ctrl+` keyboard shortcut listener
- Adjust layout for bottom panel
