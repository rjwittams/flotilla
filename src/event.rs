use crossterm::event::{EventStream, KeyEventKind};
use futures::{FutureExt, StreamExt};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub enum Event {
    Tick,
    Key(crossterm::event::KeyEvent),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<Event>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut interval = tokio::time::interval(tick_rate);
            loop {
                let delay = interval.tick();
                let event = reader.next().fuse();
                tokio::select! {
                    _ = delay => { let _ = tx.send(Event::Tick); }
                    maybe = event => match maybe {
                        Some(Ok(crossterm::event::Event::Key(k)))
                            if k.kind == KeyEventKind::Press =>
                        {
                            let _ = tx.send(Event::Key(k));
                        }
                        _ => {}
                    }
                }
            }
        });
        Self { rx }
    }

    pub async fn next(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}
