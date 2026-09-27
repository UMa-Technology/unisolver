//! Logging bridge: tracing → a Dart StreamSink.
use flutter_rust_bridge::frb;
use std::sync::{Mutex, OnceLock};

pub struct LogEventDto {
    pub level: String,
    pub target: String,
    pub message: String,
}

type Sink = crate::frb_generated::StreamSink<LogEventDto>;

static SINK: OnceLock<Mutex<Option<Sink>>> = OnceLock::new();

fn forward(level: &str, target: &str, message: &str) {
    if let Some(m) = SINK.get() {
        if let Some(sink) = m.lock().unwrap().as_ref() {
            let _ = sink.add(LogEventDto {
                level: level.to_string(),
                target: target.to_string(),
                message: message.to_string(),
            });
        }
    }
}

/// Registers the Dart log stream; the first call installs the global subscriber, later calls
/// replace the sink.
#[frb]
pub fn set_log_stream(sink: Sink) -> anyhow::Result<()> {
    let cell = SINK.get_or_init(|| Mutex::new(None));
    *cell.lock().unwrap() = Some(sink);
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        use tracing::field::{Field, Visit};
        use tracing::{Event, Subscriber};
        use tracing_subscriber::layer::{Context, Layer};
        struct MsgVisitor(String);
        impl Visit for MsgVisitor {
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    self.0 = format!("{value:?}");
                }
            }
        }
        struct FwdLayer;
        impl<S: Subscriber> Layer<S> for FwdLayer {
            fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
                let mut v = MsgVisitor(String::new());
                event.record(&mut v);
                forward(
                    &event.metadata().level().to_string(),
                    event.metadata().target(),
                    &v.0,
                );
            }
        }
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;
        let _ = tracing_subscriber::registry().with(FwdLayer).try_init();
    });
    tracing::info!("unisolver log stream attached");
    Ok(())
}
