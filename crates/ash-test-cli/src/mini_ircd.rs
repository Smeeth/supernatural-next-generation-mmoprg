// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};

#[derive(Default)]
struct MiniIrcState {
    clients: HashMap<usize, (String, mpsc::UnboundedSender<String>)>,
    channels: HashMap<String, HashSet<usize>>,
    next_id: usize,
}

/// Schlanker RFC-1459-kompatibler In-Process-IRC-Server ausschließlich für den
/// `--self-contained` Integrationstest, falls lokal kein Container-Runtime-Daemon läuft.
/// Im Docker-Setup läuft stattdessen ein echter `ngIRCd`-Container!
pub async fn start_embedded_mini_ircd(bind_addr: SocketAddr) -> Result<SocketAddr> {
    let listener = TcpListener::bind(bind_addr).await?;
    let local = listener.local_addr()?;
    let state = Arc::new(Mutex::new(MiniIrcState::default()));

    tokio::spawn(async move {
        while let Ok((stream, _peer)) = listener.accept().await {
            let st = state.clone();
            tokio::spawn(async move {
                let (r, mut w) = stream.into_split();
                let (tx, mut rx) = mpsc::unbounded_channel::<String>();

                let client_id = {
                    let mut g = st.lock().await;
                    let id = g.next_id;
                    g.next_id += 1;
                    g.clients.insert(id, (format!("guest{id}"), tx.clone()));
                    id
                };

                tokio::spawn(async move {
                    while let Some(line) = rx.recv().await {
                        let out = format!("{line}\r\n");
                        if w.write_all(out.as_bytes()).await.is_err() {
                            break;
                        }
                        let _ = w.flush().await;
                    }
                });

                let mut reader = BufReader::new(r).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let trimmed = line.trim();
                    if let Some(nick) = trimmed.strip_prefix("NICK ") {
                        let nick = nick.trim().to_string();
                        if let Some(entry) = st.lock().await.clients.get_mut(&client_id) {
                            entry.0 = nick;
                        }
                    } else if trimmed.starts_with("USER ") {
                        let nick = st
                            .lock()
                            .await
                            .clients
                            .get(&client_id)
                            .map(|(n, _)| n.clone())
                            .unwrap_or_default();
                        let _ = tx.send(format!(":ash.irc 001 {nick} :Welcome to Ash-Net IRC"));
                    } else if let Some(rest) = trimmed.strip_prefix("PING ") {
                        let _ = tx.send(format!("PONG {rest}"));
                    } else if let Some(chan) = trimmed.strip_prefix("JOIN ") {
                        let ch_name = chan.split_whitespace().next().unwrap_or("#roadhouse").to_string();
                        let mut g = st.lock().await;
                        let nick = g
                            .clients
                            .get(&client_id)
                            .map(|(n, _)| n.clone())
                            .unwrap_or_default();
                        g.channels.entry(ch_name.clone()).or_default().insert(client_id);
                        if let Some(members) = g.channels.get(&ch_name).cloned() {
                            for mid in members {
                                if let Some((_, mtx)) = g.clients.get(&mid) {
                                    let _ = mtx.send(format!(":{nick}!u@ash JOIN {ch_name}"));
                                }
                            }
                        }
                    } else if let Some(rest) = trimmed.strip_prefix("PRIVMSG ") {
                        if let Some((target, msg)) = rest.split_once(" :") {
                            let target = target.trim();
                            let g = st.lock().await;
                            let sender_nick = g
                                .clients
                                .get(&client_id)
                                .map(|(n, _)| n.clone())
                                .unwrap_or_default();
                            let wire = format!(":{sender_nick}!u@ash PRIVMSG {target} :{msg}");
                            if target.starts_with('#') {
                                if let Some(members) = g.channels.get(target) {
                                    for &mid in members {
                                        if mid != client_id {
                                            if let Some((_, mtx)) = g.clients.get(&mid) {
                                                let _ = mtx.send(wire.clone());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let mut g = st.lock().await;
                g.clients.remove(&client_id);
                for members in g.channels.values_mut() {
                    members.remove(&client_id);
                }
            });
        }
    });

    Ok(local)
}
