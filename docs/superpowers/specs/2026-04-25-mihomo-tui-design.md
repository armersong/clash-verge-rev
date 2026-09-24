# Mihomo TUI - Design Specification

**Date:** 2026-04-25
**Status:** Draft

## 1. Project Overview

- **Name:** mihomo-tui
- **Type:** Terminal UI application
- **Core functionality:** A TUI client for managing mihomo (Clash.Meta) proxy connections via its management API, featuring periodic proxy testing and auto-switching to optimal nodes.
- **Target users:** Linux/macOS users who run mihomo as a background proxy daemon and want a terminal-based management interface.

---

## 2. Architecture

### 2.1 High-Level Structure

```
mihomo-tui/
├── core/           # Core business logic
│   ├── api.rs      # Unix Socket communication with mihomo
│   ├── config.rs   # YAML config persistence
│   └── profiler.rs # Proxy latency testing
├── ui/             # ratatui rendering
│   ├── app.rs      # Main app state & event loop
│   ├── panels/     # Left nav, center list, right detail
│   └── widgets/    # Reusable UI components
└── main.rs         # Entry point
```

### 2.2 Crate Structure

- `mihomo-tui-core`: API client, config, profiler (no UI deps)
- `mihomo-tui`: Binary crate, depends on core + ratatui

### 2.3 Data Flow

1. **Config Load** → YAML config from `~/.config/mihomo-tui/config.yaml`
2. **API Connect** → Connect to mihomo Unix Socket (default: `~/.config/mihomo/socket`)
3. **Fetch Proxies** → Query mihomo API for proxy nodes
4. **Periodic Test** → Background tokio task tests proxies at user-defined interval
5. **UI Render** → ratatui displays ranked proxy list + status

---

## 3. UI Layout

### Three-Panel Layout

```
┌──────────┬─────────────────────┬──────────────────┐
│   Nav    │   Proxy List        │   Details/Logs   │
│  (10%)   │      (50%)          │      (40%)       │
├──────────┼─────────────────────┼──────────────────┤
│          │                     │                  │
│ [Sub]    │ proxy-node-1  45ms  │ Name: proxy-1    │
│ [Proxy]  │ proxy-node-2  123ms  │ Type: ss         │
│ [Test]   │ proxy-node-3  89ms   │ Delay: 45ms      │
│ [Conf]   │ ...                 │ LastTest: 10:30  │
│          │                     │                  │
├──────────┴─────────────────────┴──────────────────┤
│                    Status Bar                      │
│  Connected to mihomo │ Proxies: 25 │ AutoTest: ON  │
└───────────────────────────────────────────────────┘
```

### Navigation (Left Panel)
- `[S]` Subscriptions — manage subscription URLs
- `[P]` Proxies — view/rank proxy list
- `[T]` Test — manual trigger test
- `[C]` Config — runtime configuration

### Proxy List (Center Panel)
- Sortable by: name, delay, last test time
- Color-coded status: green (<100ms), yellow (100-500ms), red (>500ms/unreachable)

### Details/Logs (Right Panel)
- Selected proxy details
- Test logs (last 50 entries)
- Manual test output

---

## 4. Functionality Specification

### 4.1 Core Features

| Feature | Description |
|---------|-------------|
| **Subscription Import** | Paste subscription URL or raw clash config YAML, parsed and merged |
| **Proxy Display** | Fetch all proxies from mihomo API, display in sortable list |
| **Proxy Selection** | Select and activate a proxy via mihomo API |
| **Latency Testing** | TCP connect timing to each proxy's endpoint |
| **Auto-Switch** | Periodically test + auto-select optimal proxy (configurable interval) |
| **Manual Test** | Trigger immediate full test via hotkey |
| **Config Persistence** | All settings saved to YAML |

### 4.2 Scoring System (Customizable)

```yaml
scoring:
  weight_delay: 0.7      # latency weight (0.0 - 1.0)
  weight_recent: 0.3    # recency of last test weight
  timeout_ms: 5000       # test timeout per proxy
  test_interval_sec: 300 # auto-test interval (5 min default)
```

### 4.3 User Interactions

| Key | Action |
|-----|--------|
| `j/k` | Move down/up in proxy list |
| `Enter` | Select / activate proxy |
| `r` | Refresh proxy list from mihomo |
| `t` | Trigger manual test |
| `a` | Toggle auto-test on/off |
| `s` | Open subscription management |
| `c` | Open config panel |
| `q` | Quit |

### 4.4 Error Handling

- **mihomo not running:** Show error banner, prompt to start mihomo
- **Socket connection fail:** Retry with backoff (1s, 2s, 4s), then report error
- **Test timeout:** Mark proxy as unreachable, continue testing others
- **Invalid subscription URL:** Show parse error, keep previous valid config

---

## 5. Configuration

### Config File: `~/.config/mihomo-tui/config.yaml`

```yaml
mihomo:
  socket_path: "~/.config/mihomo/socket"  # Unix socket path
  api_secret: ""                            # optional API secret

scoring:
  weight_delay: 0.7
  weight_recent: 0.3
  timeout_ms: 5000
  test_interval_sec: 300

subscriptions:
  - name: "my-sub"
    url: "https://example.com/clash"

proxies:
  auto_select: true       # auto-switch to best proxy
  test_on_start: true     # run initial test on launch
```

---

## 6. Technical Dependencies

### Rust Crates

| Crate | Version | Purpose |
|-------|---------|---------|
| ratatui | 0.27 | TUI rendering |
| tokio | 1 | Async runtime |
| serde/serde_yaml | latest | YAML parsing |
| reqwest | 0.12 | HTTP for subscription fetch |
| tracing | 0.1 | Logging |

### External Dependencies

- mihomo must be running with `external-ui` and `unix-socket` or `restful-api` enabled
- Default socket: `~/.config/mihomo/socket`

---

## 7. Scope Boundaries

### In Scope
- Subscription import (manual paste URL)
- Proxy list display + selection
- Latency testing (TCP connect timing)
- Auto-switch to optimal proxy
- YAML config persistence

### Out of Scope (for this spec)
- Clash config editing (beyond subscription import)
- Rule management
- Multiple mihomo instance support
- Web UI / API server
- macOS/Windows specific integrations

---

## 8. Implementation Phases

### Phase 1: Foundation
- Project scaffold + ratatui basic layout
- mihomo API client (Unix Socket)
- Config file load/save

### Phase 2: Core Features
- Proxy list display
- Manual proxy selection
- Latency profiler

### Phase 3: Advanced Features
- Periodic auto-test
- Auto-switch logic
- Subscription import

### Phase 4: Polish
- Keybindings + navigation
- Error handling + status bar
- Testing + documentation