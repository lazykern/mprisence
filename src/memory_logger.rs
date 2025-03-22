use env_logger::fmt::Formatter;
use log::{Log, Metadata, Record, SetLoggerError};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};

static LOGGER_INITIALIZED: AtomicBool = AtomicBool::new(false);

pub struct MemoryLogger {
    inner: env_logger::Logger,
}

impl MemoryLogger {
    pub fn init() -> Result<(), SetLoggerError> {
        if LOGGER_INITIALIZED.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let env = env_logger::Env::default();
        let mut builder = env_logger::Builder::from_env(env);

        builder.format(format_with_memory);

        let logger = MemoryLogger {
            inner: builder.build(),
        };

        log::set_max_level(logger.inner.filter());
        log::set_boxed_logger(Box::new(logger))
    }
}

impl Log for MemoryLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        self.inner.log(record);
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

fn format_with_memory(buf: &mut Formatter, record: &Record) -> std::io::Result<()> {
    // Format timestamp - manually format current time
    let now = chrono::Local::now();
    let time = now.format("%Y-%m-%dT%H:%M:%S%.3f");

    // Get memory usage
    let (rss, virt) = get_memory_usage();

    // Write the log record with memory info
    let level = record.level();
    let target = record.target();
    let args = record.args();

    // Format the log entry with memory usage
    writeln!(
        buf,
        "[{} {}MB/{}MB] {} {} > {}",
        time, rss, virt, level, target, args
    )
}

#[cfg(target_os = "linux")]
fn get_memory_usage() -> (String, String) {
    use std::fs::File;
    use std::io::{BufRead, BufReader};

    let mut rss = "?".to_string();
    let mut virt = "?".to_string();

    if let Ok(file) = File::open("/proc/self/status") {
        let reader = BufReader::new(file);

        for line in reader.lines().filter_map(Result::ok) {
            if line.starts_with("VmRSS:") {
                rss = format_memory_value(&line);
            } else if line.starts_with("VmSize:") {
                virt = format_memory_value(&line);
            }

            if rss != "?" && virt != "?" {
                break;
            }
        }
    }

    (rss, virt)
}

#[cfg(not(target_os = "linux"))]
fn get_memory_usage() -> (String, String) {
    ("?".to_string(), "?".to_string())
}

fn format_memory_value(line: &str) -> String {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() >= 3 {
        if let Ok(value) = parts[1].parse::<f64>() {
            // Convert based on unit (typically kB)
            let unit = parts[2];
            let multiplier = match unit {
                "kB" => 1.0 / 1024.0, // Convert KB to MB
                "MB" => 1.0,
                "GB" => 1024.0,
                _ => 1.0 / 1024.0, // Default to KB
            };

            return format!("{:.1}", value * multiplier);
        }
    }
    "?".to_string()
}
