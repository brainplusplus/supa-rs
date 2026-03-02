# Structured Logging + HTTP TraceLayer with Request ID

**Date:** 2026-03-02
**Status:** Approved
**Scope:** Approach B — `init_tracing()` + `TraceLayer` + request ID propagation

---

## Problem

Current logging is unstructured and unconfigurable:
- `cmd_start_foreground` uses bare `tracing_subscriber::fmt::init()` (no level, no format control)
- `cmd_start_daemon_child` has inline subscriber with hardcoded defaults
- No `SUPARUST_LOG_LEVEL` or `SUPARUST_LOG_FORMAT` env vars
- Concurrent requests produce interleaved logs with no correlation ID

## Solution

Add structured, configurable logging with request ID propagation:
- Two new env vars: `SUPARUST_LOG_LEVEL` (default `info`) and `SUPARUST_LOG_FORMAT` (default `pretty`)
- Single `init_tracing()` function called by all start paths
- `tower-http` `TraceLayer` + `SetRequestIdLayer` for automatic HTTP request logging with `req_id`

---

## Dependencies

```toml
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
tower-http = { version = "0.6", features = ["trace", "request-id"] }
```

`uuid` already present with `v4` feature. `tracing-appender` deferred (see Backlog).

---

## Architecture

### Config (`src/config.rs`)

Two new fields on `Config`:

```rust
pub log_level: String,   // SUPARUST_LOG_LEVEL, default "info"
pub log_format: String,  // SUPARUST_LOG_FORMAT, default "pretty"
```

Also added to `load_or_generate_env()` auto-generated `.env` block.

### Tracing init (`src/tracing.rs`)

New file. Single public function:

```rust
pub fn init_tracing<W>(log_level: &str, log_format: &str, writer: W, ansi: bool)
where W: for<'writer> MakeWriter<'writer> + Send + Sync + 'static
```

Filter logic:
- Respects `RUST_LOG` if set (via `EnvFilter::try_from_default_env()`)
- Otherwise: `suparust={level},sqlx={sqlx_level},tower_http=debug`
  - `sqlx_level` = `debug` when level is `trace`, else `warn`

Format dispatch:
- `"json"` → `.json().with_current_span(true).with_span_list(true)`
- anything else → `.pretty().with_file(true).with_line_number(true)`

### Start paths (`src/cli/start.rs`)

Both `cmd_start_foreground` and `cmd_start_daemon_child` replace their inline subscriber setup:

```rust
// foreground
let cfg = Config::from_env();
crate::tracing::init_tracing(&cfg.log_level, &cfg.log_format, std::io::stdout(), true);

// daemon child
let log_file = ...;
crate::tracing::init_tracing(&cfg.log_level, &cfg.log_format, log_file, false);
```

`Config::from_env()` is called before `init_tracing()` so log vars are available. The existing `tracing::warn!` in `Config::from_env()` fires after subscriber is set.

### Router (`run_server()` in `src/cli/start.rs`)

```rust
use tower::ServiceBuilder;
use tower_http::request_id::{MakeRequestUuid, SetRequestIdLayer, PropagateRequestIdLayer};
use tower_http::trace::TraceLayer;

let app = Router::new()
    .nest(...)
    .layer(
        ServiceBuilder::new()
            .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(PropagateRequestIdLayer::x_request_id())
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(|req: &Request<_>| {
                        let req_id = req.headers()
                            .get("x-request-id")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("unknown");
                        tracing::info_span!(
                            "http_request",
                            req_id = %req_id,
                            method = %req.method(),
                            path   = %req.uri().path(),
                        )
                    })
            )
    );
```

Zero handler code changes. All logs within a request lifecycle inherit `req_id` via span context.

---

## File Changes

| File | Change |
|------|--------|
| `Cargo.toml` | Add `env-filter`+`json` features to `tracing-subscriber`; add `tower-http` |
| `src/config.rs` | Add `log_level`, `log_format` fields; parse from env; update auto-gen block |
| `src/tracing.rs` | **New file** — `pub fn init_tracing(...)` |
| `src/cli/start.rs` | Replace inline subscriber calls; add `TraceLayer` + request ID layers |
| `src/main.rs` | Add `mod tracing;` |
| `.env.example` | Add `SUPARUST_LOG_LEVEL=info` and `SUPARUST_LOG_FORMAT=pretty` |

---

## Log Output Examples

**`LOG_FORMAT=pretty`** (development):
```
2026-03-02T10:35:00Z  INFO http_request{req_id=a3f2 method=GET path=/rest/v1/users}: tower_http::trace: response 200 OK 12ms
2026-03-02T10:35:01Z  INFO http_request{req_id=b7c1 method=GET path=/rest/v1/posts}: tower_http::trace: response 500 3ms
2026-03-02T10:35:01Z ERROR http_request{req_id=b7c1 method=GET path=/rest/v1/posts}: suparust::api::rest: query failed: column "titl" does not exist
```

**`LOG_FORMAT=json`** (production — Vector/Loki/Datadog compatible):
```json
{"timestamp":"...","level":"ERROR","target":"suparust::api::rest","req_id":"b7c1","method":"GET","path":"/rest/v1/posts","message":"query failed: column \"titl\" does not exist"}
```

---

## Backlog

**Option C — Daily rotating log file (`tracing-appender`)**
When daemon log file (`app.log`) grows too large for production use, add `tracing_appender::rolling::daily()` as non-blocking writer with rotation. Requires adding `tracing-appender = "0.2"` dependency. Not needed now — flat `app.log` sufficient for current usage.
