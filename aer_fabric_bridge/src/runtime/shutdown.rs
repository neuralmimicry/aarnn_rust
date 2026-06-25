use tokio::sync::watch;

pub fn channel() -> (watch::Sender<bool>, watch::Receiver<bool>) {
    watch::channel(false)
}
