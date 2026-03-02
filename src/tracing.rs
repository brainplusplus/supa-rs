use tracing_subscriber::{fmt, EnvFilter};

pub enum TracingWriter {
    Stdout,
    File(std::fs::File),
}

pub fn init_tracing(log_level: &str, log_format: &str, writer: TracingWriter) {
    let sqlx_level = if log_level == "trace" { "debug" } else { "warn" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "suparust={log_level},sqlx={sqlx_level},tower_http=debug"
        ))
    });

    match (log_format, writer) {
        ("json", TracingWriter::Stdout) => {
            fmt()
                .json()
                .with_env_filter(filter)
                .with_current_span(true)
                .with_span_list(true)
                .init();
        }
        ("json", TracingWriter::File(f)) => {
            fmt()
                .json()
                .with_env_filter(filter)
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(f)
                .with_ansi(false)
                .init();
        }
        (_, TracingWriter::Stdout) => {
            fmt()
                .pretty()
                .with_env_filter(filter)
                .with_file(true)
                .with_line_number(true)
                .init();
        }
        (_, TracingWriter::File(f)) => {
            fmt()
                .pretty()
                .with_env_filter(filter)
                .with_file(true)
                .with_line_number(true)
                .with_writer(f)
                .with_ansi(false)
                .init();
        }
    }
}
