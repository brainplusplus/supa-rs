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

    let was_running = kill_pid(pid);
    std::fs::remove_file(PID_FILE).ok();

    if was_running {
        println!("SupaRust stopped (PID {})", pid);
    } else {
        println!("Process {} was not running — PID file cleaned up", pid);
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
