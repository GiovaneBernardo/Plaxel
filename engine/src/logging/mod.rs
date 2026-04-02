// Engine
#[macro_export]
macro_rules! engine_info {
    ($($arg:tt)*) => {
        tracing::info!(target: "engine", $($arg)*);
    };
}

#[macro_export]
macro_rules! engine_warn {
    ($($arg:tt)*) => {
        tracing::warn!(target: "engine", $($arg)*);
    };
}

#[macro_export]
macro_rules! engine_error {
    ($($arg:tt)*) => {
        tracing::error!(target: "engine", $($arg)*);
    };
}

// Game
#[macro_export]
macro_rules! game_info {
    ($($arg:tt)*) => {
        tracing::info!(target: "game", $($arg)*);
    };
}

#[macro_export]
macro_rules! game_warn {
    ($($arg:tt)*) => {
        tracing::warn!(target: "game", $($arg)*);
    };
}

#[macro_export]
macro_rules! game_error {
    ($($arg:tt)*) => {
        tracing::error!(target: "game", $($arg)*);
    };
}

pub fn init() {
    let filter = tracing_subscriber::EnvFilter::builder()
        .with_default_directive(tracing::Level::WARN.into())
        .parse_lossy("engine=info,game=info,wgpu=error,wgpu_hal=error,naga=error");

    tracing_subscriber::fmt().with_env_filter(filter).init();

    std::panic::set_hook(Box::new(|info| {
        eprintln!("{info}");
        eprintln!("{}", std::backtrace::Backtrace::force_capture());
    }));
}
