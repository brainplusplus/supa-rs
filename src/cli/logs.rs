use std::io::{Read, Seek, SeekFrom};

const LOG_FILE: &str = "app.log";

pub fn cmd_logs(lines: usize) {
    let mut file = match std::fs::File::open(LOG_FILE) {
        Ok(f) => f,
        Err(_) => {
            println!("No log file found — is server running in daemon mode?");
            println!("Start with: suparust start --daemon");
            return;
        }
    };

    // Read full content to get last N lines
    let content = {
        let mut buf = String::new();
        file.read_to_string(&mut buf).ok();
        buf
    };

    let all_lines: Vec<&str> = content.lines().collect();
    let start = all_lines.len().saturating_sub(lines);
    for line in &all_lines[start..] {
        println!("{}", line);
    }

    // Seek to end for follow mode
    file.seek(SeekFrom::End(0)).ok();

    // Polling follow loop — Ctrl+C to exit
    println!("--- following {} (Ctrl+C to stop) ---", LOG_FILE);
    loop {
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).ok();
        if !buf.is_empty() {
            print!("{}", String::from_utf8_lossy(&buf));
            // Flush stdout so output appears immediately (no line buffering delay)
            use std::io::Write;
            std::io::stdout().flush().ok();
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}
