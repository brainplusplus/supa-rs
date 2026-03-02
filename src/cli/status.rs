pub fn cmd_status() {
    let cfg = crate::config::Config::from_env();

    let pid_raw = std::fs::read_to_string(&cfg.pid_file)
        .map(|s| s.trim().to_string())
        .ok();

    let addr = format!("127.0.0.1:{}", cfg.port);
    let alive = std::net::TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| "127.0.0.1:3000".parse().unwrap()),
        std::time::Duration::from_secs(1),
    )
    .is_ok();

    let status_line = if alive {
        let pid_part = pid_raw.as_deref().unwrap_or("?");
        let uptime_part = uptime_from_pid_file(&cfg.pid_file).unwrap_or_default();
        format!("RUNNING  (PID {}{})", pid_part, uptime_part)
    } else {
        "STOPPED".to_string()
    };

    let base = format!("http://localhost:{}", cfg.port);
    let anon_key = std::env::var("SUPARUST_ANON_KEY")
        .unwrap_or_else(|_| "(not set — check .env)".to_string());
    let service_key = std::env::var("SUPARUST_SERVICE_KEY")
        .unwrap_or_else(|_| "(not set — check .env)".to_string());

    println!("Status:      {}", status_line);
    if alive {
        println!("API URL:     {}/rest/v1", base);
        println!("Auth URL:    {}/auth/v1", base);
        println!("Storage URL: {}/storage/v1", base);
        println!("Anon key:    {}", anon_key);
        println!("Service key: {}", service_key);
    }
}

/// Returns ", uptime Xh Ym Zs" based on pid file mtime, or "" if unavailable.
fn uptime_from_pid_file(pid_file: &str) -> Option<String> {
    let meta = std::fs::metadata(pid_file).ok()?;
    let modified = meta.modified().ok()?;
    let elapsed = modified.elapsed().ok()?;
    let s = elapsed.as_secs();
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    let uptime = if h > 0 {
        format!("{}h {}m {}s", h, m, s)
    } else if m > 0 {
        format!("{}m {}s", m, s)
    } else {
        format!("{}s", s)
    };
    Some(format!(", uptime {}", uptime))
}
