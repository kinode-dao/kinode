use tokio::signal::unix::{signal, SignalKind};

/// trigger cleanup if receive signal to kill process
pub async fn intercept_sigs() -> String {
    let mut sigalrm = signal(SignalKind::alarm()).expect("uqbar: failed to set up SIGALRM handler");
    let mut sighup = signal(SignalKind::hangup()).expect("uqbar: failed to set up SIGHUP handler");
    let mut sigint =
        signal(SignalKind::interrupt()).expect("uqbar: failed to set up SIGINT handler");
    let mut sigpipe = signal(SignalKind::pipe()).expect("uqbar: failed to set up SIGPIPE handler");
    let mut sigquit = signal(SignalKind::quit()).expect("uqbar: failed to set up SIGQUIT handler");
    let mut sigterm =
        signal(SignalKind::terminate()).expect("uqbar: failed to set up SIGTERM handler");
    let mut sigusr1 =
        signal(SignalKind::user_defined1()).expect("uqbar: failed to set up SIGUSR1 handler");
    let mut sigusr2 =
        signal(SignalKind::user_defined2()).expect("uqbar: failed to set up SIGUSR2 handler");

    tokio::select! {
        _ = sigalrm.recv() => "exiting due to SIGALRM",
        _ = sighup.recv() =>  "exiting due to SIGHUP",
        _ = sigint.recv() =>  "exiting due to SIGINT",
        _ = sigpipe.recv() => "exiting due to SIGPIPE",
        _ = sigquit.recv() => "exiting due to SIGQUIT",
        _ = sigterm.recv() => "exiting due to SIGTERM",
        _ = sigusr1.recv() => "exiting due to SIGUSR1",
        _ = sigusr2.recv() => "exiting due to SIGUSR2",
    }
    .into()
}
