use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::sleep_until;
use tokio::time::Instant;

pub fn channel<K, V>(timeout: Duration) -> (TimerSender<K, V>, TimerListener<K, V>) {
    let values = Arc::new(Mutex::new(HashMap::new()));
    let (tx, rx) = mpsc::unbounded_channel();
    let sender = TimerSender {
        timeout,
        tx,
        values: values.clone(),
    };
    let listener = TimerListener { rx, values };
    (sender, listener)
}

pub struct TimerSender<K, V> {
    timeout: Duration,
    tx: mpsc::UnboundedSender<(Instant, K)>,
    values: Arc<Mutex<HashMap<K, V>>>,
}

pub struct TimerListener<K, V> {
    rx: mpsc::UnboundedReceiver<(Instant, K)>,
    values: Arc<Mutex<HashMap<K, V>>>,
}

impl<K: Copy + Eq + Hash, V> TimerSender<K, V> {
    pub async fn trigger(&mut self, key: K, value: V) {
        let timeout = Instant::now() + self.timeout;
        let mut values = self.values.lock().await;
        values.insert(key, value);
        let _ = self.tx.send((timeout, key));
    }

    pub async fn clear(&mut self, key: K) {
        let mut values = self.values.lock().await;
        values.remove(&key);
    }
}

impl<K: Copy + Eq + Hash, V> TimerListener<K, V> {
    pub async fn timeout(&mut self) -> Option<(K, V)> {
        while let Some((timeout, key)) = self.rx.recv().await {
            sleep_until(timeout).await;
            {
                let mut values = self.values.lock().await;
                if let Some(value) = values.remove(&key) {
                    return Some((key, value));
                }
            }
        }

        None
    }
}
