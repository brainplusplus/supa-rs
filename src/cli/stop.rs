pub fn cmd_stop() {
    let cfg = crate::config::Config::from_env();
    let pid_file = &cfg.pid_file;
    let pg_port = cfg.server.port + 10_000;

    match std::fs::read_to_string(pid_file) {
        Ok(pid_str) => {
            let pid: u32 = match pid_str.trim().parse() {
                Ok(p) if p > 0 => p,
                _ => {
                    println!("Invalid PID in {} — deleting", pid_file);
                    std::fs::remove_file(pid_file).ok();
                    return;
                }
            };

            let was_running = kill_pid(pid);
            std::fs::remove_file(pid_file).ok();

            if was_running {
                wait_port_free(pg_port, 15);
                println!("SupaRust stopped (PID {})", pid);
            } else {
                println!("Process {} was not running — PID file cleaned up", pid);
            }
        }
        Err(_) => {
            // No PID file — try to find the process by port
            println!("No {} found — searching for process on port {}...", pid_file, cfg.server.port);

            match find_pid_on_port(&cfg.server.port.to_string()) {
                Some(pid) => {
                    if kill_pid(pid) {
                        wait_port_free(pg_port, 15);
                        println!("SupaRust stopped (PID {} found via port {})", pid, cfg.server.port);
                    } else {
                        println!("Process {} was not running", pid);
                    }
                }
                None => {
                    println!("No process found on port {} — server is not running", cfg.server.port);
                }
            }
        }
    }
}

/// Poll until `port` is free or `timeout_secs` elapses.
/// Uses TcpListener::bind as the lightest possible probe — no network traffic.
fn wait_port_free(port: u16, timeout_secs: u64) {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut warned = false;
    while Instant::now() < deadline {
        let free = std::net::TcpListener::bind(format!("127.0.0.1:{}", port)).is_ok();
        if free { return; }
        if !warned {
            eprintln!("[stop] Waiting for pg port {} to release...", port);
            warned = true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    eprintln!("[stop] pg port {} still busy after {}s — proceeding anyway", port, timeout_secs);
}

/// Find the PID of the process listening on the given port.
fn find_pid_on_port(port: &str) -> Option<u32> {
    #[cfg(target_os = "windows")]
    {
        // netstat -ano shows: Proto  Local  Foreign  State  PID
        let out = std::process::Command::new("netstat")
            .args(["-ano"])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let needle = format!(":{} ", port);
        for line in text.lines() {
            if line.contains(&needle) && line.to_uppercase().contains("LISTENING") {
                // Last token is PID
                if let Some(pid_str) = line.split_whitespace().last() {
                    if let Ok(pid) = pid_str.parse::<u32>() {
                        return Some(pid);
                    }
                }
            }
        }
        None
    }

    #[cfg(not(target_os = "windows"))]
    {
        // lsof -ti :PORT returns the PID directly
        let out = std::process::Command::new("lsof")
            .args(["-ti", &format!(":{}", port)])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.trim().lines().next()?.parse::<u32>().ok()
    }
}

/// Returns true if the process was actually running and killed, false if it was already dead.
fn kill_pid(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        let result = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .output();

        match result {
            Ok(output) if output.status.success() => true,
            Ok(_) => {
                // taskkill ran but failed — process was likely already dead
                false
            }
            Err(_) => {
                println!("Warning: could not spawn taskkill");
                false
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Send SIGTERM first
        let term_result = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();

        // If SIGTERM itself fails, process is probably already dead
        if let Ok(out) = &term_result {
            if !out.status.success() {
                return false;
            }
        }

        // Wait up to 5s for graceful shutdown
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let alive = std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !alive {
                return true;
            }
        }

        // Fallback: SIGKILL (fire and forget — process will die shortly)
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output();
        true
    }
}
