// SPDX-License-Identifier: GPL-3.0-or-later

use std::net::SocketAddr;

use anyhow::Result;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use ash_core::character::{CharacterClass, CharacterSheet, PlayableRace, STARTING_ZONE_ROADHOUSE};
use ash_core::error::DomainError;
use ash_db::transport::{build_quic_server, read_frame, send_response_and_close};
use ash_db::AshStore;
use ash_protocol::client::{AuthRequest, AuthResponse, ZoneRoutingTicket};
use quinn::Endpoint;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct AuthGatewayConfig {
    pub bind_addr: SocketAddr,
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
    pub roadhouse_fallback_client_addr: String,
    pub route_stale_timeout_ms: u64,
}

pub fn start_auth_gateway(config: AuthGatewayConfig, store: AshStore) -> Result<Endpoint> {
    let endpoint = build_quic_server(config.bind_addr, &config.cert_pem, &config.key_pem, None)?;
    let bound = endpoint.local_addr()?;
    info!("ash-auth-gateway listening on QUIC {bound}");

    let accept_ep = endpoint.clone();
    tokio::spawn(async move {
        while let Some(incoming) = accept_ep.accept().await {
            let store = store.clone();
            let cfg = config.clone();
            tokio::spawn(async move {
                match incoming.await {
                    Ok(conn) => {
                        while let Ok((send, mut recv)) = conn.accept_bi().await {
                            let store = store.clone();
                            let cfg = cfg.clone();
                            tokio::spawn(async move {
                                match read_frame::<AuthRequest>(&mut recv).await {
                                    Ok(req) => {
                                        let resp = handle_auth_request(req, &store, &cfg).await;
                                        let _ = send_response_and_close(send, &resp).await;
                                    }
                                    Err(err) => {
                                        warn!("auth-gateway frame read error: {err:#}");
                                    }
                                }
                            });
                        }
                    }
                    Err(err) => warn!("auth-gateway QUIC handshake failed: {err:#}"),
                }
            });
        }
    });

    Ok(endpoint)
}

async fn handle_auth_request(
    req: AuthRequest,
    store: &AshStore,
    cfg: &AuthGatewayConfig,
) -> AuthResponse {
    match req {
        AuthRequest::RegisterAccount { username, password } => {
            let username = username.trim().to_string();
            if username.len() < 3 || password.len() < 6 {
                return AuthResponse::Error {
                    code: "INVALID_CREDENTIALS".into(),
                    message: "Benutzername (min. 3) oder Passwort (min. 6) zu kurz".into(),
                };
            }
            let salt = SaltString::generate(&mut OsRng);
            let hash = match Argon2::default().hash_password(password.as_bytes(), &salt) {
                Ok(h) => h.to_string(),
                Err(e) => {
                    return AuthResponse::Error {
                        code: "HASH_ERROR".into(),
                        message: e.to_string(),
                    }
                }
            };
            match store.create_account(&username, &hash).await {
                Ok(account_id) => match store.create_session(account_id).await {
                    Ok(session_token) => AuthResponse::AccountRegistered {
                        account_id,
                        session_token,
                    },
                    Err(e) => AuthResponse::Error {
                        code: "SESSION_ERROR".into(),
                        message: e.to_string(),
                    },
                },
                Err(e) => AuthResponse::Error {
                    code: "ACCOUNT_EXISTS".into(),
                    message: e.to_string(),
                },
            }
        }
        AuthRequest::Login { username, password } => {
            let Ok(Some(acc)) = store.find_account_by_username(username.trim()).await else {
                return AuthResponse::Error {
                    code: "AUTH_FAILED".into(),
                    message: "Unbekannter Account oder falsches Passwort".into(),
                };
            };
            let valid = PasswordHash::new(&acc.password_hash)
                .ok()
                .and_then(|parsed| {
                    Argon2::default()
                        .verify_password(password.as_bytes(), &parsed)
                        .ok()
                })
                .is_some();
            if !valid {
                return AuthResponse::Error {
                    code: "AUTH_FAILED".into(),
                    message: "Unbekannter Account oder falsches Passwort".into(),
                };
            }
            match store.create_session(acc.id).await {
                Ok(session_token) => AuthResponse::LoggedIn {
                    account_id: acc.id,
                    session_token,
                },
                Err(e) => AuthResponse::Error {
                    code: "SESSION_ERROR".into(),
                    message: e.to_string(),
                },
            }
        }
        AuthRequest::CreateCharacter {
            session_token,
            name,
            race,
            class,
        } => {
            let Some(account_id) = store.resolve_session(&session_token).await.ok().flatten()
            else {
                return AuthResponse::Error {
                    code: "UNAUTHORIZED".into(),
                    message: "Ungültige oder abgelaufene Session".into(),
                };
            };
            let parsed_race = match PlayableRace::parse_strict(&race) {
                Ok(r) => r,
                Err(DomainError::ForbiddenSupernaturalOrigin(origin)) => {
                    return AuthResponse::Error {
                        code: "FORBIDDEN_OVERPOWER_RACE".into(),
                        message: format!(
                            "Engel und Dämonen ('{origin}') sind als Spieler-Rasse strikt gesperrt!"
                        ),
                    }
                }
                Err(e) => {
                    return AuthResponse::Error {
                        code: "INVALID_RACE".into(),
                        message: e.to_string(),
                    }
                }
            };
            let parsed_class = match CharacterClass::parse_strict(&class) {
                Ok(c) => c,
                Err(e) => {
                    return AuthResponse::Error {
                        code: "INVALID_CLASS".into(),
                        message: e.to_string(),
                    }
                }
            };

            let hunter = match CharacterSheet::create_new_hunter(
                account_id,
                name.trim(),
                parsed_race,
                parsed_class,
            ) {
                Ok(h) => h,
                Err(e) => {
                    return AuthResponse::Error {
                        code: "MATRIX_VIOLATION".into(),
                        message: e.to_string(),
                    }
                }
            };

            if let Err(e) = store.save_character(&hunter).await {
                return AuthResponse::Error {
                    code: "DB_SAVE_ERROR".into(),
                    message: e.to_string(),
                };
            }
            AuthResponse::CharacterCreated { character: hunter }
        }
        AuthRequest::ListCharacters { session_token } => {
            let Some(account_id) = store.resolve_session(&session_token).await.ok().flatten()
            else {
                return AuthResponse::Error {
                    code: "UNAUTHORIZED".into(),
                    message: "Ungültige Session".into(),
                };
            };
            match store.list_characters_for_account(account_id).await {
                Ok(characters) => AuthResponse::CharacterList { characters },
                Err(e) => AuthResponse::Error {
                    code: "DB_ERROR".into(),
                    message: e.to_string(),
                },
            }
        }
        AuthRequest::IssueZoneTicket {
            session_token,
            character_id,
        } => {
            let Some(account_id) = store.resolve_session(&session_token).await.ok().flatten()
            else {
                return AuthResponse::Error {
                    code: "UNAUTHORIZED".into(),
                    message: "Ungültige Session".into(),
                };
            };
            let Ok(Some(mut ch)) = store.load_character(character_id).await else {
                return AuthResponse::Error {
                    code: "NOT_FOUND".into(),
                    message: "Charakter nicht gefunden".into(),
                };
            };
            if ch.account_id != account_id {
                return AuthResponse::Error {
                    code: "FORBIDDEN".into(),
                    message: "Charakter gehört nicht zu diesem Account".into(),
                };
            }

            // Prüfe Live-Routing-Tabelle; ist der Ziel-Zonenknoten abgestürzt/offline,
            // greift die Ausfallsicherheit: sicherer Rücktransport nach `zone_roadhouse`!
            let (target_zone, target_client_addr, fallback_to_roadhouse) = match store
                .get_active_zone_route(&ch.current_zone, cfg.route_stale_timeout_ms)
                .await
                .ok()
                .flatten()
            {
                Some(route) => (route.zone_id, route.client_public_addr, false),
                None => {
                    let was_other_zone = ch.current_zone != STARTING_ZONE_ROADHOUSE;
                    if was_other_zone {
                        let _ = store.fallback_character_to_roadhouse(&mut ch).await;
                    }
                    let roadhouse_addr = store
                        .get_active_zone_route(STARTING_ZONE_ROADHOUSE, cfg.route_stale_timeout_ms)
                        .await
                        .ok()
                        .flatten()
                        .map(|r| r.client_public_addr)
                        .unwrap_or_else(|| cfg.roadhouse_fallback_client_addr.clone());
                    (
                        STARTING_ZONE_ROADHOUSE.to_string(),
                        roadhouse_addr,
                        was_other_zone,
                    )
                }
            };

            let ticket = ZoneRoutingTicket {
                ticket_token: Uuid::new_v4().to_string(),
                character_id: ch.id,
                target_zone,
                target_client_addr,
                fallback_to_roadhouse,
            };
            if let Err(e) = store.store_zone_ticket(&ticket).await {
                return AuthResponse::Error {
                    code: "TICKET_ERROR".into(),
                    message: e.to_string(),
                };
            }
            AuthResponse::ZoneTicketIssued { ticket }
        }
    }
}
