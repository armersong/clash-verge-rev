# Mihomo TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a TUI application for mihomo (Clash.Meta) proxy management featuring proxy testing, auto-switching, and subscription import.

**Architecture:** Two-crate workspace: `mihomo-tui-core` (API client, config, profiler - no UI deps) and `mihomo-tui` (binary crate with ratatui). Three-panel layout (nav, proxy list, details) with background tokio tasks for periodic testing.

**Tech Stack:** Rust, ratatui 0.27, tokio, serde_yaml, reqwest, tracing

**Project Location:** `~/projects/rust/mihomo-tui/`

---

## File Structure

```
mihomo-tui/
├── Cargo.toml                  # workspace
├── mihomo-tui-core/
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs              # re-exports
│       ├── api.rs             # Unix Socket API client
│       ├── config.rs          # YAML config persistence
│       └── profiler.rs         # Proxy latency testing
└── mihomo-tui/
    ├── Cargo.toml
    └── src/
        ├── main.rs            # entry, async init
        ├── app.rs             # AppState, run loop
        └── ui/
            ├── mod.rs
            ├── panels/
            │   ├── mod.rs
            │   ├── nav.rs      # left nav panel
            │   ├── proxy.rs    # center proxy list
            │   └── detail.rs   # right detail panel
            └── widgets/
                ├── mod.rs
                └── status.rs   # status bar widget
```

---

## Task 1: Project Scaffold

**Files:**
- Create: `~/projects/rust/mihomo-tui/Cargo.toml`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui-core/Cargo.toml`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui-core/src/lib.rs`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/Cargo.toml`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/src/main.rs`

- [ ] **Step 1: Create workspace Cargo.toml**

```toml
[workspace]
members = ["mihomo-tui-core", "mihomo-tui"]
resolver = "2"
```

- [ ] **Step 2: Create mihomo-tui-core/Cargo.toml**

```toml
[package]
name = "mihomo-tui-core"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_yaml = "0.9"
reqwest = { version = "0.12", features = ["json"] }
tracing = "0.1"
thiserror = "2"
dirs = "6"
```

- [ ] **Step 3: Create mihomo-tui-core/src/lib.rs**

```rust
pub mod api;
pub mod config;
pub mod profiler;
pub mod subscription;

pub use api::{MihomoClient, ProxyNode};
pub use config::AppConfig;
pub use profiler::{Profiler, ProxyTestResult};
pub use subscription::fetch_subscription;
```

- [ ] **Step 4: Create mihomo-tui/Cargo.toml**

```toml
[package]
name = "mihomo-tui"
version = "0.1.0"
edition = "2021"

[dependencies]
mihomo-tui-core = { path = "../mihomo-tui-core" }
ratatui = "0.27"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
anyhow = "1"
crossterm = "0.27"
```

- [ ] **Step 5: Create mihomo-tui/src/main.rs**

```rust
mod app;
mod ui;

use app::App;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut app = App::new().await?;
    app.run_with_events()?;
    Ok(())
}
```

- [ ] **Step 6: Verify build**

Run: `cd ~/projects/rust/mihomo-tui && cargo build`
Expected: Compiles successfully

- [ ] **Step 7: Commit**

```bash
cd ~/projects/rust/mihomo-tui
git init
git add -A
git commit -m "chore: project scaffold with two-crate workspace"
```

---

## Task 2: Config Module

**Files:**
- Create: `~/projects/rust/mihomo-tui/mihomo-tui-core/src/config.rs`

- [ ] **Step 1: Define Config struct**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub mihomo: MihomoConfig,
    pub scoring: ScoringConfig,
    pub subscriptions: Vec<Subscription>,
    pub proxies: ProxyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MihomoConfig {
    pub socket_path: String,
    pub api_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoringConfig {
    pub weight_delay: f32,
    pub weight_recent: f32,
    pub timeout_ms: u64,
    pub test_interval_sec: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub auto_select: bool,
    pub test_on_start: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mihomo: MihomoConfig {
                socket_path: "~/.config/mihomo/socket".to_string(),
                api_secret: String::new(),
            },
            scoring: ScoringConfig {
                weight_delay: 0.7,
                weight_recent: 0.3,
                timeout_ms: 5000,
                test_interval_sec: 300,
            },
            subscriptions: Vec::new(),
            proxies: ProxyConfig {
                auto_select: true,
                test_on_start: true,
            },
        }
    }
}
```

- [ ] **Step 2: Implement load/save**

```rust
use std::fs;
use std::path::PathBuf;

impl AppConfig {
    fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mihomo-tui")
            .join("config.yaml")
    }

    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::config_path();
        if !path.exists() {
            let default = AppConfig::default();
            default.save()?;
            return Ok(default);
        }
        let content = fs::read_to_string(&path)?;
        serde_yaml::from_str(&content).map_err(ConfigError::Parse)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_yaml::to_string(self).map_err(ConfigError::Serialize)?;
        fs::write(&path, content).map_err(ConfigError::Io)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(serde_yaml::Error),
    #[error("serialize error: {0}")]
    Serialize(serde_yaml::Error),
}
```

- [ ] **Step 3: Verify build**

Run: `cd ~/projects/rust/mihomo-tui && cargo build`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(core): add config module with load/save"
```

---

## Task 3: API Client

**Files:**
- Create: `~/projects/rust/mihomo-tui/mihomo-tui-core/src/api.rs`

- [ ] **Step 1: Define types**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyNode {
    pub name: String,
    #[serde(rename = "type")]
    pub proxy_type: String,
    pub endpoint: Option<String>,
    pub udp: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyListResponse {
    pub proxies: Vec<ProxyNode>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("request failed: {0}")]
    Request(#[from] std::io::Error),
    #[error("parse error: {0}")]
    Parse(serde_json::Error),
}
```

- [ ] **Step 2: Implement MihomoClient**

```rust
use std::path::Path;

pub struct MihomoClient {
    socket_path: String,
    secret: String,
}

impl MihomoClient {
    pub fn new(socket_path: String, secret: String) -> Self {
        Self { socket_path, secret }
    }

    fn expand_socket_path(path: &str) -> String {
        if path.starts_with('~') {
            dirs::home_dir()
                .map(|h| path.replacen("~", h.to_str().unwrap_or(""), 1))
                .unwrap_or_else(|| path.to_string())
        } else {
            path.to_string()
        }
    }

    pub async fn get_proxies(&self) -> Result<Vec<ProxyNode>, ApiError> {
        use tokio::net::UnixStream;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let path = Self::expand_socket_path(&self.socket_path);
        let mut stream = UnixStream::connect(&path).await
            .map_err(|e| ApiError::Connection(e.to_string()))?;

        let body = if self.secret.is_empty() {
            "GET /proxies HTTP/1.1\r\nHost: localhost\r\n\r\n".to_string()
        } else {
            format!(
                "GET /proxies HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {}\r\n\r\n",
                self.secret
            )
        };

        stream.write_all(body.as_bytes()).await?;
        let mut buf = vec![0u8; 8192];
        let n = stream.read(&mut buf).await?;

        let response = String::from_utf8_lossy(&buf[..n]);
        let json_start = response.find("\r\n\r\n").map(|i| i + 4);
        if let Some(start) = json_start {
            let json_str = &response[start..];
            let resp: ProxyListResponse = serde_json::from_str(json_str)
                .map_err(ApiError::Parse)?;
            Ok(resp.proxies)
        } else {
            Err(ApiError::Connection("Invalid response".to_string()))
        }
    }

    pub async fn select_proxy(&self, name: &str) -> Result<(), ApiError> {
        use tokio::net::UnixStream;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let path = Self::expand_socket_path(&self.socket_path);
        let mut stream = UnixStream::connect(&path).await
            .map_err(|e| ApiError::Connection(e.to_string()))?;

        let body = format!(
            "PUT /proxies/{} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\n\r\n{{\"name\":\"{}\"}}",
            name, name
        );

        stream.write_all(body.as_bytes()).await?;
        let mut buf = vec![0u8; 256];
        let _n = stream.read(&mut buf).await?;

        Ok(())
    }

    pub async fn get_proxies_with_retry(&self, max_attempts: u32) -> Result<Vec<ProxyNode>, ApiError> {
        let mut attempts = 0;
        loop {
            match self.get_proxies().await {
                Ok(proxies) => return Ok(proxies),
                Err(e) if attempts < max_attempts => {
                    attempts += 1;
                    let delay = 2u64.pow(attempts) * 1000;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }
}
```

Add `serde_json` to Cargo.toml.

- [ ] **Step 3: Update lib.rs**

```rust
pub mod api;
pub mod config;
pub mod profiler;
pub mod subscription;

pub use api::{MihomoClient, ProxyNode, ApiError};
pub use config::AppConfig;
pub use profiler::{Profiler, ProxyTestResult};
pub use subscription::fetch_subscription;
```

- [ ] **Step 4: Verify build**

Run: `cd ~/projects/rust/mihomo-tui && cargo build`
Expected: Compiles successfully

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(core): add MihomoClient for Unix Socket API"
```

---

## Task 4: Profiler (Latency Testing)

**Files:**
- Create: `~/projects/rust/mihomo-tui/mihomo-tui-core/src/profiler.rs`

- [ ] **Step 1: Define types**

```rust
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct ProxyTestResult {
    pub name: String,
    pub delay_ms: Option<u64>,
    pub tested_at: DateTime<Utc>,
    pub score: f32,
}
```

Add `chrono` to Cargo.toml.

- [ ] **Step 2: Implement Profiler**

```rust
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

pub struct Profiler;

impl Profiler {
    pub async fn test_all(proxies: &[ProxyNode], config: &ScoringConfig) -> Vec<ProxyTestResult> {
        let mut handles = Vec::new();

        for proxy in proxies {
            let endpoint = proxy.endpoint.clone().unwrap_or_default();
            let timeout_ms = config.timeout_ms;

            handles.push(tokio::spawn(async move {
                let delay = Self::test_single(&endpoint, timeout_ms).await;
                ProxyTestResult {
                    name: proxy.name.clone(),
                    delay_ms: delay,
                    tested_at: Utc::now(),
                    score: 0.0,
                }
            }));
        }

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }
        results
    }

    async fn test_single(endpoint: &str, timeout_ms: u64) -> Option<u64> {
        if endpoint.is_empty() {
            return None;
        }

        let addr = endpoint.to_socket_addrs().ok()?.next()?;
        let start = std::time::Instant::now();
        let result = timeout(
            Duration::from_millis(timeout_ms),
            TcpStream::connect(addr)
        ).await;

        if let Ok(Ok(_)) = result {
            Some(start.elapsed().as_millis() as u64)
        } else {
            None
        }
    }

    pub fn rank_results(results: &mut Vec<ProxyTestResult>, config: &ScoringConfig) {
        for result in results.iter_mut() {
            result.score = Self::calc_score(result, config);
        }
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    }

    fn calc_score(result: &ProxyTestResult, config: &ScoringConfig) -> f32 {
        let delay_score = result.delay_ms
            .map(|d| (5000 - d.min(5000)) as f32 / 5000.0)
            .unwrap_or(0.0);

        let recent_secs = Utc::now().signed_duration_since(result.tested_at).num_seconds() as f32;
        let recency_score = (600.0 - recent_secs.min(600.0)) / 600.0;

        delay_score * config.weight_delay + recency_score * config.weight_recent
    }
}
```

- [ ] **Step 3: Verify build**

Run: `cd ~/projects/rust/mihomo-tui && cargo build`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(core): add Profiler for proxy latency testing"
```

---

## Task 5: Subscription Module

**Files:**
- Create: `~/projects/rust/mihomo-tui/mihomo-tui-core/src/subscription.rs`

- [ ] **Step 1: Create subscription.rs**

```rust
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub async fn fetch_subscription(url: &str) -> Result<Vec<ProxyNode>, SubscriptionError> {
    let response = reqwest::get(url).await
        .map_err(SubscriptionError::Fetch)?
        .text().await
        .map_err(SubscriptionError::Read)?;

    let yaml_content = if response.len() > 50 && !response.contains("proxies:") {
        if let Ok(decoded) = STANDARD.decode(&response) {
            String::from_utf8(decoded).map_err(|_| SubscriptionError::Parse)?
        } else {
            response
        }
    } else {
        response
    };

    let config: serde_yaml::Value = serde_yaml::from_str(&yaml_content)
        .map_err(SubscriptionError::Parse)?;

    let proxies = parse_clash_proxies(&config)?;
    Ok(proxies)
}

fn parse_clash_proxies(config: &serde_yaml::Value) -> Result<Vec<ProxyNode>, SubscriptionError> {
    let proxies_arr = config.get("proxies")
        .and_then(|p| p.as_sequence())
        .ok_or(SubscriptionError::Parse)?;

    let proxies = proxies_arr.iter().filter_map(|p| {
        Some(ProxyNode {
            name: p.get("name")?.as_str()?.to_string(),
            proxy_type: p.get("type")?.as_str()?.to_string(),
            endpoint: {
                let server = p.get("server")?.as_str()?;
                let port = p.get("port")?.as_str()?;
                Some(format!("{}:{}", server, port))
            },
            udp: p.get("udp").and_then(|u| u.as_bool()).unwrap_or(false),
        })
    }).collect();

    Ok(proxies)
}

#[derive(Debug, thiserror::Error)]
pub enum SubscriptionError {
    #[error("fetch error: {0}")]
    Fetch(reqwest::Error),
    #[error("read error: {0}")]
    Read(reqwest::Error),
    #[error("parse error: {0}")]
    Parse(serde_yaml::Error),
}
```

Add `base64` to Cargo.toml.

- [ ] **Step 2: Verify build**

Run: `cd ~/projects/rust/mihomo-tui && cargo build`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat(core): add subscription module with base64 decode"
```

---

## Task 6: ratatui App Shell with Three-Panel Layout

**Files:**
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/src/ui/mod.rs`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/src/ui/panels/mod.rs`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/src/ui/widgets/mod.rs`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/src/app.rs`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/src/ui/widgets/status.rs`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/src/ui/panels/nav.rs`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/src/ui/panels/proxy.rs`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/src/ui/panels/detail.rs`
- Create: `~/projects/rust/mihomo-tui/mihomo-tui/src/ui/panels/subscription.rs`

- [ ] **Step 1: Create ui/mod.rs**

```rust
pub mod panels;
pub mod widgets;
```

- [ ] **Step 2: Create ui/panels/mod.rs**

```rust
pub mod nav;
pub mod proxy;
pub mod detail;
pub mod subscription;
```

- [ ] **Step 3: Create ui/widgets/mod.rs**

```rust
pub mod status;
```

- [ ] **Step 4: Create ui/widgets/status.rs**

```rust
use ratatui::prelude::*;
use chrono::{DateTime, Utc};

pub fn render_status_bar(f: &mut Frame, area: Rect, status: &StatusBarData) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(25),
            Constraint::Min(10),
            Constraint::Length(25),
        ])
        .split(area);

    let connected_text = if status.connected { "Connected" } else { "Disconnected" };
    f.render_widget(
        Line::from(connected_text).style(if status.connected {
            Style::new().fg(Color::Green)
        } else {
            Style::new().fg(Color::Red)
        }),
        chunks[0],
    );
    f.render_widget(
        Line::from(format!("Proxies: {}", status.proxy_count)),
        chunks[1],
    );
    let auto_text = if status.auto_test { "AutoTest: ON" } else { "AutoTest: OFF" };
    f.render_widget(
        Line::from(auto_text).style(Style::new().fg(if status.auto_test { Color::Green } else { Color::Yellow })),
        chunks[2],
    );
}

#[derive(Debug, Clone)]
pub struct StatusBarData {
    pub connected: bool,
    pub proxy_count: usize,
    pub auto_test: bool,
}
```

- [ ] **Step 5: Create ui/panels/nav.rs**

```rust
use ratatui::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NavItem {
    Subscriptions,
    Proxies,
    Test,
    Config,
}

pub fn render_nav(f: &mut Frame, area: Rect, active: NavItem) {
    let items = vec![
        ("[S] Subscriptions", NavItem::Subscriptions),
        ("[P] Proxies", NavItem::Proxies),
        ("[T] Test", NavItem::Test),
        ("[C] Config", NavItem::Config),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Navigation ");
    f.render_widget(block, area);

    let inner = area.inner(Margin { x: 1, y: 1 });
    for (i, (label, item)) in items.iter().enumerate() {
        let style = if *item == active {
            Style::new().fg(Color::Yellow).bold()
        } else {
            Style::new().fg(Color::White)
        };
        f.render_widget(
            Line::from(*label).style(style),
            Rect::new(inner.x, inner.y + i as u16, inner.width, 1),
        );
    }
}
```

- [ ] **Step 6: Create ui/panels/proxy.rs**

```rust
use ratatui::prelude::*;
use mihomo_tui_core::ProxyTestResult;

pub fn render_proxy_list(f: &mut Frame, area: Rect, results: &[ProxyTestResult], selected: usize) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Proxy List ");
    f.render_widget(block, area);

    let inner = area.inner(Margin { x: 1, y: 1 });

    for (i, result) in results.iter().enumerate().take(inner.height as usize) {
        let color = result.delay_ms
            .map(|d| if d < 100 { Color::Green } else if d < 500 { Color::Yellow } else { Color::Red })
            .unwrap_or(Color::Red);

        let delay_str = result.delay_ms.map(|d| format!("{}ms", d)).unwrap_or_else(|| "timeout".to_string());

        let prefix = if i == selected { ">" } else { " " };
        let line = Line::from(format!(
            "{} {:40} {:>10} {:>8.4}",
            prefix,
            result.name,
            delay_str,
            result.score
        )).style(Style::new().fg(color));

        f.render_widget(line, Rect::new(inner.x, inner.y + i as u16, inner.width.saturating_sub(1), 1));
    }
}
```

- [ ] **Step 7: Create ui/panels/detail.rs**

```rust
use ratatui::prelude::*;
use mihomo_tui_core::ProxyTestResult;

pub fn render_detail(f: &mut Frame, area: Rect, result: Option<&ProxyTestResult>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Details ");
    f.render_widget(block, area);

    let inner = area.inner(Margin { x: 1, y: 1 });

    if let Some(r) = result {
        let lines = vec![
            Line::from(format!("Name: {}", r.name)),
            Line::from(format!("Delay: {}ms", r.delay_ms.unwrap_or(0))),
            Line::from(format!("Score: {:.4}", r.score)),
            Line::from(format!("Tested: {}", r.tested_at.format("%H:%M:%S"))),
        ];
        for (i, line) in lines.iter().enumerate() {
            f.render_widget(*line, Rect::new(inner.x, inner.y + i as u16, inner.width, 1));
        }
    } else {
        f.render_widget(
            Line::from("No proxy selected").fg(Color::White),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
    }
}
```

- [ ] **Step 8: Create ui/panels/subscription.rs**

```rust
use ratatui::prelude::*;

pub fn render_subscription(f: &mut Frame, area: Rect, state: &SubscriptionPanelState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Subscription ");
    f.render_widget(block, area);

    let inner = area.inner(Margin { x: 1, y: 1 });

    if let Some(msg) = &state.message {
        f.render_widget(
            Line::from(msg.as_str()),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
    } else if !state.subscriptions.is_empty() {
        for (i, sub) in state.subscriptions.iter().enumerate().take(inner.height as usize) {
            f.render_widget(
                Line::from(format!("{} - {}", sub.name, sub.url)),
                Rect::new(inner.x, inner.y + i as u16, inner.width, 1),
            );
        }
    } else {
        f.render_widget(
            Line::from("No subscriptions. Press 'a' to add."),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
    }
}

#[derive(Debug, Default)]
pub struct SubscriptionPanelState {
    pub subscriptions: Vec<super::Subscription>,
    pub message: Option<String>,
}
```

- [ ] **Step 9: Create app.rs**

```rust
use ratatui::{Terminal, backend::CrosstermBackend, Event};
use std::io::stdout;
use mihomo_tui_core::{AppConfig, MihomoClient, Profiler, ProxyTestResult};
use crate::ui::panels::{nav, proxy, detail, subscription, NavItem};
use crate::ui::widgets::status;

pub struct App {
    config: AppConfig,
    client: MihomoClient,
    proxies: Vec<ProxyTestResult>,
    selected_index: usize,
    active_nav: NavItem,
    connection_error: Option<String>,
    subscription_state: subscription::SubscriptionPanelState,
}

impl App {
    pub async fn new() -> anyhow::Result<Self> {
        let config = AppConfig::load().unwrap_or_default();
        let client = MihomoClient::new(config.mihomo.socket_path.clone(), config.mihomo.api_secret.clone());

        let mut app = Self {
            config,
            client,
            proxies: Vec::new(),
            selected_index: 0,
            active_nav: NavItem::Proxies,
            connection_error: None,
            subscription_state: Default::default(),
        };

        if app.config.proxies.test_on_start {
            app.refresh_proxies().await.ok();
        }

        if app.config.proxies.auto_select {
            app.start_auto_test().await;
        }

        Ok(app)
    }

    pub fn run_with_events(&mut self) -> anyhow::Result<()> {
        let backend = CrosstermBackend::new(stdout());
        let mut terminal = Terminal::new(backend)?;
        let events = ratatui::events::Events::new();

        loop {
            terminal.draw(|f| self.render(f))?;

            match events.next()? {
                Event::Key(key) => match key.code {
                    ratatui::events::KeyCode::Char('q') => break,
                    ratatui::events::KeyCode::Char('j') => {
                        self.selected_index = self.selected_index.saturating_add(1).min(self.proxies.len().saturating_sub(1));
                    }
                    ratatui::events::KeyCode::Char('k') => {
                        self.selected_index = self.selected_index.saturating_sub(1);
                    }
                    ratatui::events::KeyCode::Char('s') => self.active_nav = NavItem::Subscriptions,
                    ratatui::events::KeyCode::Char('p') => self.active_nav = NavItem::Proxies,
                    ratatui::events::KeyCode::Char('t') => self.active_nav = NavItem::Test,
                    ratatui::events::KeyCode::Char('c') => self.active_nav = NavItem::Config,
                    ratatui::events::KeyCode::Char('r') => {
                        let app = self;
                        tokio::spawn(async move {
                            app.refresh_proxies().await;
                        });
                    }
                    ratatui::events::KeyCode::Char('a') => {
                        self.config.proxies.auto_select = !self.config.proxies.auto_select;
                        self.config.save().ok();
                    }
                    ratatui::events::KeyCode::Enter => {
                        let app = self;
                        let name = app.proxies.get(app.selected_index).map(|p| p.name.clone());
                        if let Some(n) = name {
                            tokio::spawn(async move {
                                app.client.select_proxy(&n).await.ok();
                            });
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        Ok(())
    }

    fn render(&self, f: &mut Frame) {
        let size = f.size();
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(10),
                Constraint::Percentage(50),
                Constraint::Percentage(40),
            ])
            .split(size);

        let nav_area = chunks[0];
        let proxy_area = chunks[1];
        let detail_area = chunks[2];

        nav::render_nav(f, nav_area, self.active_nav);
        proxy::render_proxy_list(f, proxy_area, &self.proxies, self.selected_index);

        let selected_result = self.proxies.get(self.selected_index);
        detail::render_detail(f, detail_area, selected_result);

        let status_data = status::StatusBarData {
            connected: self.connection_error.is_none(),
            proxy_count: self.proxies.len(),
            auto_test: self.config.proxies.auto_select,
        };
        let status_area = Rect::new(size.x, size.y.saturating_sub(1), size.width, 1);
        status::render_status_bar(f, status_area, &status_data);
    }

    async fn refresh_proxies(&mut self) {
        match self.client.get_proxies_with_retry(3).await {
            Ok(proxy_nodes) => {
                let mut results = Profiler::test_all(&proxy_nodes, &self.config.scoring).await;
                Profiler::rank_results(&mut results, &self.config.scoring);
                self.proxies = results;
                self.connection_error = None;
            }
            Err(e) => {
                self.connection_error = Some(e.to_string());
            }
        }
    }

    async fn start_auto_test(&self) {
        let interval_sec = self.config.scoring.test_interval_sec;
        let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(interval_sec));
        let config = self.config.clone();
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                ticker.tick().await;
                if let Ok(proxies) = client.get_proxies().await {
                    let mut results = Profiler::test_all(&proxies, &config.scoring).await;
                    Profiler::rank_results(&mut results, &config.scoring);
                    if config.proxies.auto_select {
                        if let Some(best) = results.first() {
                            client.select_proxy(&best.name).await.ok();
                        }
                    }
                }
            }
        });
    }
}
```

- [ ] **Step 10: Verify build**

Run: `cd ~/projects/rust/mihomo-tui && cargo build`
Expected: Compiles successfully

- [ ] **Step 11: Commit**

```bash
git add -A
git commit -m "feat(ui): add three-panel ratatui layout with all panels and key handling"
```

---

## Task 7: README + Final Polish

**Files:**
- Create: `~/projects/rust/mihomo-tui/README.md`
- Create: `~/projects/rust/mihomo-tui/.gitignore`

- [ ] **Step 1: Create .gitignore**

```
/target
**/*.rs.bk
*.swp
```

- [ ] **Step 2: Create README.md**

```markdown
# mihomo-tui

A TUI client for mihomo (Clash.Meta) proxy management.

## Features

- Three-panel terminal UI (navigation, proxy list, details)
- Periodic proxy latency testing with customizable scoring
- Auto-switch to optimal proxy
- Subscription import (base64 encoded clash config)
- YAML configuration persistence

## Requirements

- mihomo running with Unix Socket API enabled
- `socket_path` in config pointing to mihomo socket

## Usage

```bash
# Build
cargo build --release

# Run
./target/release/mihomo-tui
```

## Keybindings

| Key | Action |
|-----|--------|
| `j/k` | Navigate proxy list |
| `Enter` | Select/activate proxy |
| `r` | Refresh proxy list |
| `t` | Switch to Test panel |
| `a` | Toggle auto-test |
| `s` | Open subscriptions |
| `c` | Open config |
| `q` | Quit |

## Configuration

Config stored at `~/.config/mihomo-tui/config.yaml`:

```yaml
mihomo:
  socket_path: "~/.config/mihomo/socket"
  api_secret: ""

scoring:
  weight_delay: 0.7
  weight_recent: 0.3
  timeout_ms: 5000
  test_interval_sec: 300

proxies:
  auto_select: true
  test_on_start: true
```
```

- [ ] **Step 3: Verify final build**

Run: `cd ~/projects/rust/mihomo-tui && cargo build --release`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: add README and final polish"
git tag v0.1.0
```

---

## Spec Coverage Check

| Spec Requirement | Task |
|------------------|------|
| Three-panel layout | Task 6 |
| Proxy list display | Task 6 |
| Proxy selection via mihomo API | Task 3, Task 6 |
| Latency testing (TCP timing) | Task 4 |
| Scoring (customizable weight) | Task 2, Task 4 |
| Periodic auto-test | Task 6 |
| Auto-switch to optimal | Task 6 |
| Manual test trigger | Task 6 |
| Subscription import (URL, base64) | Task 5 |
| YAML config persistence | Task 2 |
| Keybindings | Task 6 |
| Error handling + retry | Task 3 |
| Status bar | Task 6 |

All requirements covered. No gaps.

---

## Plan Complete

**Plan saved to:** `docs/superpowers/plans/2026-04-25-mihomo-tui-plan.md`

**Project location:** `~/projects/rust/mihomo-tui/`

Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?