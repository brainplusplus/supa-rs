const PID_FILE: &str = ".suparust.pid";

pub fn cmd_stop() {
    let pid_str = match std::fs::read_to_string(PID_FILE) {
        Ok(s) => s,
        Err(_) => {
            println!("No server running (no {} found)", PID_FILE);
            return;
        }
    };

    let pid: u32 = match pid_str.trim().parse() {
        Ok(p) if p > 0 => p,
        _ => {
            println!("Invalid PID in {} — deleting", PID_FILE);
            std::fs::remove_file(PID_FILE).ok();
            return;
        }
    };

    kill_pid(pid);
    std::fs::remove_file(PID_FILE).ok();
    println!("SupaRust stopped (PID {})", pid);
}

fn kill_pid(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let result = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F", "/T"])
            .output();
        if result.is_err() {
            println!("Warning: taskkill failed — process may already be gone");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Send SIGTERM first
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output();

        // Wait up to 5s for graceful shutdown
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            // Check if still alive by sending signal 0
            let alive = std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !alive {
                return;
            }
        }

        // Fallback: SIGKILL
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .output();
    }
}
