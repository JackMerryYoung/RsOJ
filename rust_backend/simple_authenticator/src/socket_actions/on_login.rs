use crate::global::*;

use mysql_async::prelude::*;
use rand::Rng;

#[derive(serde::Deserialize, serde::Serialize)]
struct ContentOnLogin {
    username: String,
    password: String,
    request_key: String,
}

mod msg_to_send_generator {
    #[derive(serde::Deserialize, serde::Serialize)]
    struct ContentInQuitOnLoginFailure {
        reason: String,
        request_key: String,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    struct ContentInSessionTokenOnLoginSuccess {
        session_token: String,
        request_key: String,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    pub struct SessionTokenOnLoginSuccess {
        r#type: String,
        content: ContentInSessionTokenOnLoginSuccess,
    }

    #[derive(serde::Deserialize, serde::Serialize)]
    pub struct QuitOnLoginFailure {
        r#type: String,
        content: ContentInQuitOnLoginFailure,
    }

    pub fn generate_quit_on_login_failure(request_key: String) -> QuitOnLoginFailure {
        QuitOnLoginFailure {
            r#type: String::from("quit"),
            content: ContentInQuitOnLoginFailure {
                reason: String::from("authentication_failure"),
                request_key,
            },
        }
    }

    pub fn generate_session_token_on_login_success(
        session_token: String,
        request_key: String,
    ) -> SessionTokenOnLoginSuccess {
        SessionTokenOnLoginSuccess {
            r#type: String::from("session_token"),
            content: ContentInSessionTokenOnLoginSuccess {
                session_token,
                request_key,
            },
        }
    }
}

async fn send_json_msg_to_ws_server_on_login_failure(
    original_ws_id: String,
    unwrapped_content: ContentOnLogin,
) {
    let json_msg: SocketJsonMessage = SocketJsonMessage {
        r#type: String::from("on_send_msg"),
        content: serde_json::to_value(SocketJsonMessageContentOnSendMsg {
            ws_id: original_ws_id,
            msg_to_send: serde_json::to_value(
                msg_to_send_generator::generate_quit_on_login_failure(
                    unwrapped_content.request_key,
                ),
            )
            .unwrap(),
        })
        .unwrap(),
        request_key: uuid::Uuid::new_v4().to_string(),
        from_protocol: String::from("std_authenticator"),
    };
    let json_msg_value = serde_json::to_value(json_msg).unwrap();
    send_socket_json_message(&json_msg_value, "std_ws_server").await;
}

async fn send_json_msg_to_ws_server_on_login_success(
    original_ws_id: String,
    unwrapped_content: ContentOnLogin,
    session_token: String,
) {
    let json_msg: SocketJsonMessage = SocketJsonMessage {
        r#type: String::from("on_send_msg"),
        content: serde_json::to_value(SocketJsonMessageContentOnSendMsg {
            ws_id: original_ws_id,
            msg_to_send: serde_json::to_value(
                msg_to_send_generator::generate_session_token_on_login_success(
                    session_token,
                    unwrapped_content.request_key,
                ),
            )
            .unwrap(),
        })
        .unwrap(),
        request_key: uuid::Uuid::new_v4().to_string(),
        from_protocol: String::from("std_authenticator"),
    };
    let json_msg_value = serde_json::to_value(json_msg).unwrap();
    send_socket_json_message(&json_msg_value, "std_ws_server").await;
}

pub async fn on_login(msg: SocketJsonMessageWithWsId) {
    let Ok(unwrapped_content) = serde_json::from_value::<ContentOnLogin>(msg.content) else {
        // There is no reliable request key when the payload itself is malformed, so there is no
        // useful response we can route back to the browser. Keep malformed traffic from taking
        // down the authenticator task, however.
        eprintln!(
            "[{}] [WARNING] Ignoring malformed login request.",
            MODULE_IDENTITY
        );
        return;
    };

    let password_hash: String = get_hash(unwrapped_content.password.as_str());
    let mut conn: mysql_async::Conn = match get_db_conn().await {
        Ok(conn) => conn,
        Err(error) => {
            eprintln!(
                "[{}] [WARNING] Failed to connect to the database while logging in `{}`: {}",
                MODULE_IDENTITY, unwrapped_content.username, error
            );
            send_json_msg_to_ws_server_on_login_failure(msg.ws_id, unwrapped_content).await;
            return;
        }
    };
    let results: Result<Vec<String>, _> = conn
        .exec(
            "SELECT password FROM RsOJ.users WHERE username = :username",
            mysql_async::params! { "username" => &unwrapped_content.username },
        )
        .await;
    drop(conn);
    match results {
        Ok(results_unwrapped) => {
            if let Some(real_password_hash) = results_unwrapped.first() {
                if real_password_hash == &password_hash {
                    let new_session_token: String =
                        generate_session_token(rand::rng().random_range(u32::MAX / 4..=u32::MAX));

                    let _session_state = SESSION_STATE_LOCK.lock().await;
                    unbind_ws_identity(&msg.ws_id).await;

                    println!(
                            "{}",
                            ansi_term::Color::Blue.paint(
                                format!(
                                    "[{}] [INFO] [THREAD {}] [FILE `{}` LINE {}] The user `{}` logged in successfully.",
                                    MODULE_IDENTITY,
                                    std::thread::current().id().as_u64(),
                                    file!(),
                                    line!(),
                                    unwrapped_content.username
                                )
                            )
                        );
                    let mut guard_logged_in_usernames: tokio::sync::MutexGuard<
                        '_,
                        std::collections::HashSet<String>,
                    > = LOGGED_IN_USERNAMES.lock().await;
                    guard_logged_in_usernames.insert(unwrapped_content.username.clone());
                    drop(guard_logged_in_usernames);

                    let old_ws_id = WS_IDS_BY_USERNAME
                        .lock()
                        .await
                        .insert(unwrapped_content.username.clone(), msg.ws_id.clone());

                    let mut guard_usernames_by_ws_id: tokio::sync::MutexGuard<
                        '_,
                        std::collections::HashMap<String, String>,
                    > = USERNAMES_BY_WS_ID.lock().await;
                    if let Some(old_ws_id) = old_ws_id
                        && old_ws_id != msg.ws_id
                    {
                        guard_usernames_by_ws_id.remove(&old_ws_id);
                    }
                    guard_usernames_by_ws_id
                        .insert(msg.ws_id.clone(), unwrapped_content.username.clone());
                    drop(guard_usernames_by_ws_id);

                    let mut guard_session_tokens: tokio::sync::MutexGuard<
                        '_,
                        std::collections::HashMap<String, String>,
                    > = SESSION_TOKENS_BY_USERNAME.lock().await;
                    guard_session_tokens.insert(
                        unwrapped_content.username.clone(),
                        new_session_token.clone(),
                    );
                    drop(guard_session_tokens);
                    drop(_session_state);
                    send_json_msg_to_ws_server_on_login_success(
                        msg.ws_id,
                        unwrapped_content,
                        new_session_token,
                    )
                    .await;
                } else {
                    println!(
                            "{}",
                            ansi_term::Color::Yellow.paint(
                                format!(
                                    "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] The user `{}` failed to login (The user tried to login with a wrong password).",
                                    MODULE_IDENTITY,
                                    std::thread::current().id().as_u64(),
                                    file!(),
                                    line!(),
                                    unwrapped_content.username
                                )
                            )
                        );
                    send_json_msg_to_ws_server_on_login_failure(msg.ws_id, unwrapped_content).await;
                }
            } else {
                println!(
                        "{}",
                        ansi_term::Color::Yellow.paint(
                            format!(
                                "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] The user `{}` failed to login (The user doesn't exist).",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!(),
                                unwrapped_content.username
                            )
                        )
                    );
                send_json_msg_to_ws_server_on_login_failure(msg.ws_id, unwrapped_content).await;
            }
        }
        Err(e) => {
            println!(
                    "{}",
                    ansi_term::Color::Yellow.paint(
                        format!(
                            "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] The user `{}` failed to login (The SQL query is not correct).",
                            MODULE_IDENTITY,
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!(),
                            unwrapped_content.username
                        )
                    )
                );

            println!(
                "{}",
                ansi_term::Color::Yellow.paint(format!(
                    "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] {}",
                    MODULE_IDENTITY,
                    std::thread::current().id().as_u64(),
                    file!(),
                    line!(),
                    e
                ))
            );
            // Always complete the request. Without a response the request broker waits for
            // its timeout, which makes a transient database failure look like a broken login
            // to the user.
            send_json_msg_to_ws_server_on_login_failure(msg.ws_id, unwrapped_content).await;
        }
    }
}
