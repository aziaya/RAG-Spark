//! Terminal setup, background tasks (live metrics and trace stream) and the event loop.

use crate::{
    app::{App, AppMsg},
    client::Client,
    sessions::SessionStore,
    ui,
};
use anyhow::Result;
use crossterm::event::EventStream;
use futures_util::StreamExt;
use rag_types::wire::ServerConfig;
use ratatui::DefaultTerminal;
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

pub async fn run(client: Client, cfg: ServerConfig, sessions: SessionStore) -> Result<()> {
    let (tx, mut rx) = unbounded_channel::<AppMsg>();
    spawn_background(&client, &cfg, &tx);
    let server_url = client.base_url().to_string();
    let mut app = App::new(client, cfg, sessions, server_url, tx);
    // `init` enters the alternate screen + raw mode and installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, &mut rx).await;
    ratatui::restore();
    result
}

/// Poll the server's metrics every second (this is also the connection check) and follow its trace
/// stream, reconnecting if it drops. Both stop when the UI exits.
fn spawn_background(client: &Client, cfg: &ServerConfig, tx: &UnboundedSender<AppMsg>) {
    let (c, t) = (client.clone(), tx.clone());
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tick.tick().await;
            let r = c.metrics().await.map_err(|e| format!("{e:#}"));
            if t.send(AppMsg::Metrics(r)).is_err() {
                break;
            }
        }
    });
    if cfg.events_enabled {
        let (c, t) = (client.clone(), tx.clone());
        tokio::spawn(async move {
            loop {
                let t2 = t.clone();
                let _ = c
                    .stream_events(move |e| {
                        let _ = t2.send(AppMsg::Trace(e));
                    })
                    .await;
                if t.is_closed() {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }
}

async fn event_loop(terminal: &mut DefaultTerminal, app: &mut App, rx: &mut UnboundedReceiver<AppMsg>) -> Result<()> {
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        terminal.draw(|f| ui::draw(f, app))?;
        tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(ev)) => app.on_event(ev),
                Some(Err(e)) => return Err(e.into()),
                None => break,
            },
            Some(msg) = rx.recv() => {
                app.on_msg(msg);
                while let Ok(m) = rx.try_recv() {
                    app.on_msg(m);
                }
            }
            _ = tick.tick() => app.on_tick(),
        }
        if app.should_quit {
            break;
        }
    }
    Ok(())
}
