pub fn cmd_status() {
    dotenvy::dotenv().ok();
    let port = std::env::var("SUPARUST_PORT")
        .or_else(|_| std::env::var("PORT"))
        .unwrap_or_else(|_| "3000".to_string());

    let pid = std::fs::read_to_string(".suparust.pid")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "(no pid file)".to_string());

    let addr = format!("127.0.0.1:{}", port);
    let alive = std::net::TcpStream::connect_timeout(
        &addr.parse().unwrap_or_else(|_| "127.0.0.1:3000".parse().unwrap()),
        std::time::Duration::from_secs(1),
    )
    .is_ok();

    println!("PID:    {}", pid);
    println!("Port:   {}", port);
    println!("Status: {}", if alive { "RUNNING" } else { "STOPPED" });
}
