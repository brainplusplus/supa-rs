# SupaRust Observability Integration Guide

SupaRust emits structured JSON logs — no embedded log shipper required. The single binary writes
everything to stdout, so any log pipeline that can read a stream or a file works out of the box.

## Log Format

```json
{"timestamp":"2026-03-02T10:00:00.000Z","level":"INFO","target":"suparust::api::rest","req_id":"a1b2c3","method":"GET","path":"/rest/v1/users","message":"request completed"}
```

Key environment variables:

| Variable | Default | Description |
|---|---|---|
| `SUPARUST_LOG_FORMAT` | `json` | `json` or `pretty` |
| `SUPARUST_LOG_LEVEL` | `info` | `error`, `warn`, `info`, `debug`, `trace` |
| `RUST_LOG` | _(unset)_ | Fine-grained override (see below) |

---

## Debug Override with RUST_LOG

`RUST_LOG` is evaluated before `SUPARUST_LOG_LEVEL` and supports per-crate filtering:

```bash
# Debug only SupaRust code + sqlx queries
RUST_LOG=suparust=debug,sqlx=debug ./suparust start

# Trace everything (very noisy)
RUST_LOG=trace ./suparust start

# Quiet third-party crates, verbose SupaRust
RUST_LOG=warn,suparust=debug ./suparust start
```

---

## 1. Vector

Vector replaces the Vector container from the Supabase Docker Compose stack. Point it at
SupaRust's stdout (via a log file or stdin) and route to any sink.

### File source → HTTP sink (e.g. Loki, Datadog, any HTTP endpoint)

```toml
# vector.toml

[sources.suparust_logs]
type = "file"
include = ["/var/log/suparust/app.log"]
read_from = "beginning"

[transforms.parse_json]
type = "remap"
inputs = ["suparust_logs"]
source = '''
. = parse_json!(string!(.message))
.service = "suparust"
'''

[sinks.http_out]
type = "http"
inputs = ["parse_json"]
uri = "http://localhost:3100/loki/api/v1/push"
encoding.codec = "json"
```

### Stdin source → console (quick local test)

```toml
# vector.toml

[sources.stdin_in]
type = "stdin"

[sinks.console_out]
type = "console"
inputs = ["stdin_in"]
encoding.codec = "json"
```

```bash
./suparust start | vector --config vector.toml
```

---

## 2. Grafana Loki + Promtail

Promtail reads from the log file written by `suparust start --daemon` (default: `app.log`).

```yaml
# promtail-config.yaml

server:
  http_listen_port: 9080

positions:
  filename: /tmp/positions.yaml

clients:
  - url: http://localhost:3100/loki/api/v1/push

scrape_configs:
  - job_name: suparust
    static_configs:
      - targets:
          - localhost
        labels:
          job: suparust
          host: __hostname__
          __path__: /var/log/suparust/app.log
    pipeline_stages:
      - json:
          expressions:
            level: level
            target: target
            req_id: req_id
      - labels:
          level:
          target:
      - timestamp:
          source: timestamp
          format: RFC3339
```

```bash
promtail -config.file=promtail-config.yaml
```

---

## 3. Datadog

### Option A: Vector → Datadog sink

```toml
# vector.toml

[sources.suparust_logs]
type = "file"
include = ["/var/log/suparust/app.log"]

[transforms.tag]
type = "remap"
inputs = ["suparust_logs"]
source = '''
. = parse_json!(string!(.message))
.service = "suparust"
.env = "production"
'''

[sinks.datadog]
type = "datadog_logs"
inputs = ["tag"]
default_api_key = "${DD_API_KEY}"
site = "datadoghq.com"
```

### Option B: Datadog Agent log collection

```yaml
# /etc/datadog-agent/conf.d/suparust.d/conf.yaml

logs:
  - type: file
    path: /var/log/suparust/app.log
    service: suparust
    source: rust
    tags:
      - env:production
```

```bash
# Restart agent after adding config
systemctl restart datadog-agent
```

---

## 4. systemd journald

Run SupaRust as a managed systemd service so all output is captured by journald automatically.

```ini
# /etc/systemd/system/suparust.service

[Unit]
Description=SupaRust — self-hosted Supabase backend
After=network.target

[Service]
Type=simple
User=suparust
WorkingDirectory=/opt/suparust
EnvironmentFile=/opt/suparust/.env
ExecStart=/opt/suparust/suparust start
Restart=on-failure
RestartSec=5

# Send stdout/stderr to journald
StandardOutput=journal
StandardError=journal
SyslogIdentifier=suparust

[Install]
WantedBy=multi-user.target
```

```bash
# Enable and start
systemctl daemon-reload
systemctl enable --now suparust

# Stream logs
journalctl -u suparust -f

# Stream as JSON (pipe to jq, Vector, etc.)
journalctl -u suparust -f -o json

# Query by log level field (journald captures structured fields from JSON output)
journalctl -u suparust -o json | jq 'select(.level == "ERROR")'
```

---

## Docker Usage

When running SupaRust in a container, merge stderr into stdout and pipe to your shipper:

```bash
docker run --rm suparust 2>&1 | vector --config vector.toml
```

Or with Docker Compose, use the `json-file` log driver and point Promtail at the Docker log path:

```yaml
# docker-compose.yml (excerpt)
services:
  suparust:
    image: suparust
    logging:
      driver: json-file
      options:
        max-size: "50m"
        max-file: "5"
```

```yaml
# promtail-config.yaml (Docker variant)
scrape_configs:
  - job_name: suparust
    static_configs:
      - labels:
          job: suparust
          __path__: /var/lib/docker/containers/*/*-json.log
    pipeline_stages:
      - json:
          expressions:
            log: log
      - json:
          source: log
          expressions:
            level: level
            target: target
```
