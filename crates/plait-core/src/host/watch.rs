use std::{
    future::poll_fn,
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
};

use super::{Client, Error, Snapshot};

#[derive(Default)]
struct Inbox {
    latest: Option<Result<Snapshot, Error>>,
    closed: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
struct Delivery(Mutex<Inbox>);
impl Delivery {
    fn publish(&self, value: Result<Snapshot, Error>) -> bool {
        let mut inbox = self.0.lock().unwrap();
        if inbox.closed {
            return false;
        }
        inbox.closed = value.is_err();
        let keep = !inbox.closed;
        // Snapshots replace state. A slow UI needs the latest one, not an event backlog.
        inbox.latest = Some(value);
        let waker = inbox.waker.take();
        drop(inbox);
        if let Some(waker) = waker {
            waker.wake();
        }
        keep
    }
}

/// Dropping any cancellation handle closes only the subscription, never the run.
#[derive(Clone)]
pub struct CancelWatch(Arc<UnixStream>);
impl Drop for CancelWatch {
    fn drop(&mut self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

pub struct Snapshots {
    delivery: Arc<Delivery>,
    cancel: CancelWatch,
}
impl Snapshots {
    pub fn cancel_handle(&self) -> CancelWatch {
        self.cancel.clone()
    }

    pub async fn next(&mut self) -> Option<Result<Snapshot, Error>> {
        poll_fn(|cx| {
            let mut inbox = self.delivery.0.lock().unwrap();
            if let Some(value) = inbox.latest.take() {
                return Poll::Ready(Some(value));
            }
            if inbox.closed {
                return Poll::Ready(None);
            }
            inbox.waker = Some(cx.waker().clone());
            Poll::Pending
        })
        .await
    }
}
impl Drop for Snapshots {
    fn drop(&mut self) {
        let mut inbox = self.delivery.0.lock().unwrap();
        inbox.closed = true;
        inbox.latest = None;
        inbox.waker = None;
        // The cancellation field then wakes the blocked reader so it can release its thread.
    }
}

impl Client {
    pub fn subscribe(&self, thread_id: String, protocol: u32) -> Result<Snapshots, Error> {
        Snapshots::start(self.watch(thread_id, protocol)?)
    }
}

impl Snapshots {
    fn start(watch: super::Watch) -> Result<Self, Error> {
        let cancel = CancelWatch(Arc::new(watch.frames.reader.get_ref().try_clone()?));
        let delivery = Arc::new(Delivery::default());
        let writer = Arc::clone(&delivery);
        // An idle blocking read must not occupy GPUI's shared background worker pool.
        std::thread::Builder::new()
            .name("plait-host-watch".into())
            .spawn(move || {
                let mut watch = watch;
                while writer.publish(watch.next_snapshot()) {}
            })?;
        Ok(Snapshots { delivery, cancel })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{Frames, Watch};
    use std::{
        io::BufReader,
        sync::mpsc,
        time::{Duration, Instant},
    };

    fn reader() -> (Watch, CancelWatch, UnixStream) {
        let (socket, peer) = UnixStream::pair().unwrap();
        let cancel = CancelWatch(Arc::new(socket.try_clone().unwrap()));
        let watch = Watch {
            frames: Frames {
                reader: BufReader::new(socket),
                pending: Vec::new(),
                partial_since: None,
            },
            thread_id: "thread".into(),
            identity: None,
            first: false,
            initial_deadline: Instant::now() + Duration::from_secs(15),
        };
        (watch, cancel, peer)
    }

    #[test]
    fn idle_read_stays_asleep_until_cancel_and_sends_no_stop_request() {
        use std::io::Read;
        let (mut watch, cancel, mut peer) = reader();
        let (send, receive) = mpsc::channel();
        let reader = std::thread::spawn(move || send.send(watch.next_snapshot()).unwrap());
        assert!(matches!(
            receive.recv_timeout(Duration::from_millis(1150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(cancel);
        assert!(
            receive
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .is_err()
        );
        let mut bytes = [0; 64];
        assert_eq!(peer.read(&mut bytes).unwrap(), 0);
        reader.join().unwrap();
    }

    #[test]
    fn initial_snapshot_still_has_an_absolute_deadline() {
        let (mut watch, _cancel, _peer) = reader();
        watch.first = true;
        watch.initial_deadline = Instant::now() + Duration::from_millis(20);
        assert!(watch.next_snapshot().is_err());
    }

    #[test]
    fn async_delivery_wakes_once_and_receiver_drop_closes_the_socket() {
        use std::{
            future::Future,
            io::Read,
            sync::atomic::{AtomicUsize, Ordering},
            task::{Context, Wake},
        };
        struct Counter(AtomicUsize);
        impl Wake for Counter {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let (_watch, cancel, mut peer) = reader();
        let delivery = Arc::new(Delivery::default());
        let mut snapshots = Snapshots {
            delivery: Arc::clone(&delivery),
            cancel,
        };
        let counter = Arc::new(Counter(AtomicUsize::new(0)));
        let waker = Waker::from(Arc::clone(&counter));
        let mut cx = Context::from_waker(&waker);
        {
            let mut next = std::pin::pin!(snapshots.next());
            assert!(next.as_mut().poll(&mut cx).is_pending());
            assert!(!delivery.publish(Err(Error::Transport("closed".into()))));
            assert_eq!(counter.0.load(Ordering::SeqCst), 1);
            assert!(matches!(
                next.as_mut().poll(&mut cx),
                Poll::Ready(Some(Err(_)))
            ));
        }
        assert!(matches!(
            std::pin::pin!(snapshots.next()).as_mut().poll(&mut cx),
            Poll::Ready(None)
        ));
        drop(snapshots);
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    }

    #[test]
    fn subscription_decodes_initial_and_combined_event_frames() {
        use std::{
            future::Future,
            io::{Read, Write},
            task::{Context, Wake},
        };
        struct WakeThread(std::thread::Thread);
        impl Wake for WakeThread {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }
        let (mut watch, _cancel, mut peer) = reader();
        watch.first = true;
        let mut snapshots = Snapshots::start(watch).unwrap();
        let snapshot = serde_json::json!({
            "id":"thread", "checkout":"/repo", "model":{"provider":"p", "model_id":"m"},
            "status":"running", "messages":[], "live_text":"initial", "tools":[],
            "pending_approval":null, "error":null, "truncated":false
        });
        let reply = serde_json::json!({"ok":true,"result":{"thread":snapshot.clone()}});
        let mut latest = snapshot;
        latest["live_text"] = "latest".into();
        let event = serde_json::json!({"event":"snapshot","thread":latest});
        peer.write_all(format!("{reply}\n{event}\n").as_bytes())
            .unwrap();
        let waker = Waker::from(Arc::new(WakeThread(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let result = {
                let mut next = std::pin::pin!(snapshots.next());
                loop {
                    match next.as_mut().poll(&mut cx) {
                        Poll::Ready(value) => break value.unwrap().unwrap(),
                        Poll::Pending => {
                            assert!(Instant::now() < deadline, "subscription did not wake");
                            std::thread::park_timeout(Duration::from_millis(10));
                        }
                    }
                }
            };
            if result.live_text == "latest" {
                break;
            }
            assert_eq!(result.live_text, "initial");
        }
        drop(snapshots);
        assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
    }

    #[test]
    fn identity_violations_are_rejected_before_async_coalescing() {
        use std::io::Write;
        for field in ["id", "checkout", "model"] {
            let (mut watch, _cancel, mut peer) = reader();
            watch.first = true;
            let snapshot = serde_json::json!({
                "id":"thread", "checkout":"/repo", "model":{"provider":"p", "model_id":"m"},
                "status":"running", "messages":[], "live_text":"", "tools":[],
                "pending_approval":null, "error":null, "truncated":false
            });
            let first = serde_json::json!({"ok":true,"result":{"thread":snapshot.clone()}});
            let mut changed = snapshot.clone();
            changed[field] = if field == "model" {
                serde_json::json!({"provider":"p","model_id":"wrong"})
            } else {
                "/wrong".into()
            };
            let bad = serde_json::json!({"event":"snapshot","thread":changed});
            let good = serde_json::json!({"event":"snapshot","thread":snapshot});
            peer.write_all(format!("{first}\n{bad}\n{good}\n").as_bytes())
                .unwrap();
            assert!(watch.next_snapshot().is_ok());
            assert!(matches!(watch.next_snapshot(), Err(Error::Protocol(_))));
        }
    }

    #[test]
    fn slow_consumers_keep_one_latest_snapshot_and_observe_failure() {
        let delivery = Delivery::default();
        for index in 0..1000 {
            let snapshot: Snapshot = serde_json::from_value(serde_json::json!({
                "id":"thread", "checkout":"/repo", "model":{"provider":"p", "model_id":"m"},
                "status":"running", "messages":[], "live_text":index.to_string(), "tools":[],
                "pending_approval":null, "error":null, "truncated":false
            }))
            .unwrap();
            assert!(delivery.publish(Ok(snapshot)));
        }
        assert_eq!(
            delivery
                .0
                .lock()
                .unwrap()
                .latest
                .take()
                .unwrap()
                .unwrap()
                .live_text,
            "999"
        );
        assert!(!delivery.publish(Err(Error::Transport("disconnected".into()))));
        assert!(delivery.0.lock().unwrap().latest.take().unwrap().is_err());
        assert!(!delivery.publish(Err(Error::Transport("again".into()))));
        assert!(delivery.0.lock().unwrap().latest.is_none());
    }
}
