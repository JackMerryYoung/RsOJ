#![feature(thread_id_value)]

mod global;
mod ws_handler;
mod socket_actions;
mod self_management;
mod ws_server_socket;
mod avatar_routes;

use std::io::Read;

use global::*;

#[derive(serde::Deserialize, serde::Serialize)]
struct ExternalListenerConfigJson {
    command: String,
    required_protocol_version: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct WsServerConfigJson {
    restricted_mode: bool,
    external_listeners_list: Vec<ExternalListenerConfigJson>,
}

#[unsafe(no_mangle)]
pub extern "Rust" fn on_init(
    global_module_statuses_by_protocol: AsyncModifiable<
        std::collections::HashMap<String, AsyncModifiable<ModuleStatus>>
    >
) -> (tokio::runtime::Runtime, AsyncModifiable<ModuleStatus>) {
    let ws_server_runtime: tokio::runtime::Runtime = tokio::runtime::Builder
        ::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    GLOBAL_MODULE_STATUSES_BY_PROTOCOL.set(global_module_statuses_by_protocol.clone()).unwrap();
    let ws_server_status: ModuleStatus = ModuleStatus {
        initialized: false,
        panicked: false,
        socket_port: new_async_modifiable(0),
        init_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
    };
    let ws_server_status: AsyncModifiable<ModuleStatus> = new_async_modifiable(ws_server_status);
    // Initialization
    {
        let ws_server_status: AsyncModifiable<ModuleStatus> = ws_server_status.clone();
        let failure_status = ws_server_status.clone();
        let initialization_handle = ws_server_runtime.spawn(async move {
            // Try to load config file.
            let mut ws_server_config_json_string: String = String::new();
            let ws_server_config_json_file_path: String =
                get_pwd() + "/ws_server/ws_server_config_rs.json";
            let mut ws_server_config_json_file: std::fs::File = match
                std::fs::File::open(&ws_server_config_json_file_path)
            {
                Ok(file) => file,
                Err(e) => {
                    eprintln!(
                        "{}",
                        ansi_term::Color::Red.paint(
                            format!(
                                "[{}] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Failed to open module_config_rs.json from `{}`. Maybe the file doesn't exist?",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!(),
                                ws_server_config_json_file_path
                            )
                        )
                    );
                    eprintln!(
                        "{}",
                        ansi_term::Color::Red.paint(
                            format!(
                                "[{}] [ERROR] [THREAD {}] [FILE `{}` LINE {}] {}",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!(),
                                e
                            )
                        )
                    );
                    on_unload(0);
                    let mut guard_ws_server_status: tokio::sync::MutexGuard<
                        '_,
                        ModuleStatus
                    > = ws_server_status.lock().await;
                    guard_ws_server_status.panicked = true;
                    guard_ws_server_status.init_notify.notify_waiters();
                    drop(guard_ws_server_status); // Avoid poisoning the mutex lock.
                    return;
                }
            };
            ws_server_config_json_file.read_to_string(&mut ws_server_config_json_string).unwrap();
            let ws_server_config_json: WsServerConfigJson = serde_json
                ::from_str(ws_server_config_json_string.as_str())
                .unwrap();
            let mut ws_server_external_listeners_by_command: std::collections::HashMap<
                String,
                ExternalListener
            > = std::collections::HashMap::new();
            for external_listener_config in ws_server_config_json.external_listeners_list {
                let required_protocol_version: semver::VersionReq = semver::VersionReq::parse(
                    &external_listener_config.required_protocol_version
                ).unwrap_or_else(|e| {
                    eprintln!(
                        "{}",
                        ansi_term::Color::Red.paint(
                            format!(
                                "[{}] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Invalid `required_protocol_version` (`{}`) for command `{}` in ws_server_config_rs.json: {}",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!(),
                                external_listener_config.required_protocol_version,
                                external_listener_config.command,
                                e
                            )
                        )
                    );
                    panic!();
                });
                ws_server_external_listeners_by_command.insert(
                    external_listener_config.command.clone(),
                    ExternalListener {
                        command: external_listener_config.command,
                        required_protocol_version,
                        protocols: std::collections::HashMap::new(),
                    }
                );
            }
            WS_SERVER_EXTERNAL_LISTENERS_BY_COMMAND.set(
                std::sync::RwLock::new(std::sync::Arc::new(ws_server_external_listeners_by_command))
            ).unwrap();
            // Try to establish a socket for messaging.
            let ws_server_socket: tokio::net::TcpListener;
            let mut ws_server_socket_port: u16 = 9000;
            let (tmp_socket, tmp_port) = loop {
                let ws_server_socket_result: Result<
                    tokio::net::TcpListener,
                    std::io::Error
                > = tokio::net::TcpListener::bind(
                    format!("127.0.0.1:{}", ws_server_socket_port)
                ).await;
                if let Ok(x) = ws_server_socket_result {
                    println!(
                        "{}",
                        ansi_term::Color::Green.paint(
                            format!(
                                "[{}] [INFO] [THREAD {}] [FILE `{}` LINE {}] Initialized the socket on port {}.",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!(),
                                ws_server_socket_port
                            )
                        )
                    );
                    break (x, ws_server_socket_port);
                } else {
                    println!(
                        "{}",
                        ansi_term::Color::Yellow.paint(
                            format!(
                                "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] Failed to open the socket on port {}. Retrying...",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!(),
                                ws_server_socket_port
                            )
                        )
                    );
                }
                if ws_server_socket_port == u16::MAX {
                    eprintln!(
                        "{}",
                        ansi_term::Color::Red.paint(
                            format!(
                                "[{}] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Exceeded maximum retry times. Now quitting... ",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!()
                            )
                        )
                    );
                    panic!();
                }
                ws_server_socket_port += 1;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            };
            let guard_ws_server_status: tokio::sync::MutexGuard<
                '_,
                ModuleStatus
            > = ws_server_status.lock().await; // Get the status of the server.
            let mut guard_ws_server_status_socket_port: tokio::sync::MutexGuard<
                '_,
                u16
            > = guard_ws_server_status.socket_port.lock().await;
            (ws_server_socket, *guard_ws_server_status_socket_port) = (tmp_socket, tmp_port);
            drop(guard_ws_server_status_socket_port);
            drop(guard_ws_server_status);
            WS_SERVER_SOCKET.set(new_async_modifiable(ws_server_socket)).unwrap();
            println!(
                "{}",
                ansi_term::Color::Blue.paint(
                    format!(
                        "[{}] [INFO] [THREAD {}] [FILE `{}` LINE {}] Initializing the Websocket server...",
                        MODULE_IDENTITY,
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!()
                    )
                )
            );
            let ws_server_app: axum::Router = axum::Router
                ::new()
                .route(
                    "/",
                    axum::routing::get(|| async { "Rust Backend Test" })
                )
                .route("/avatar/{username}", axum::routing::get(crate::avatar_routes::serve_avatar))
                .route(
                    "/avatar/upload",
                    // Generous headroom over the 1MB file-size limit enforced inside the
                    // handler itself, so a slightly-too-big file gets a clean error message
                    // from our own check rather than the body-limit layer aborting the request.
                    axum::routing::post(crate::avatar_routes::upload_avatar).layer(
                        axum::extract::DefaultBodyLimit::max(4 * 1024 * 1024)
                    )
                )
                .merge(
                    axum::Router
                        ::new()
                        .route("/ws", axum::routing::get(crate::ws_handler::ws_handler))
                        .layer(
                            tower::ServiceBuilder
                                ::new()
                                .layer(axum_client_ip::ClientIpSource::ConnectInfo.into_extension())
                                .layer(axum::middleware::from_fn(crate::ws_handler::ip_handler))
                        )
                );
            // A previous instance may still be releasing 9983 during a graceful restart.
            // Keep retrying briefly instead of failing the whole module immediately.
            let listener: Result<tokio::net::TcpListener, std::io::Error> = retry!(
                tokio::net::TcpListener::bind("0.0.0.0:9983").await,
                20,
                1000,
                {
                    println!(
                        "{}",
                        ansi_term::Color::Yellow.paint(
                            format!(
                                "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] Failed to initialize the Websocket server. Retrying...",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!()
                            )
                        )
                    );
                }
            );
            let listener: tokio::net::TcpListener = match listener {
                Ok(listener) => {
                    println!(
                        "{}",
                        ansi_term::Color::Green.paint(
                            format!(
                                "[{}] [INFO] [THREAD {}] [FILE `{}` LINE {}] Initialized the Websocket server.",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!()
                            )
                        )
                    );

                    let mut guard_ws_server_status: tokio::sync::MutexGuard<
                        '_,
                        ModuleStatus
                    > = ws_server_status.lock().await;
                    guard_ws_server_status.initialized = true;
                    // notify_waiters(), not notify_one(): there are two independent waiters on
                    // this same init_notify — main_backend's module-loading wait, and this
                    // module's own internal wait below before it starts processing its socket.
                    // notify_one() only wakes one of them, permanently starving the other.
                    guard_ws_server_status.init_notify.notify_waiters();
                    drop(guard_ws_server_status);
                    listener
                }
                Err(_) => {
                    eprintln!(
                        "{}",
                        ansi_term::Color::Red.paint(
                            format!(
                                "[{}] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Exceeded maximum retry times. Now quitting...",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!()
                            )
                        )
                    );

                    on_unload(0);
                    let mut guard_ws_server_status: tokio::sync::MutexGuard<
                        '_,
                        ModuleStatus
                    > = ws_server_status.lock().await;
                    guard_ws_server_status.panicked = true;
                    guard_ws_server_status.init_notify.notify_waiters();
                    drop(guard_ws_server_status); // Avoid poisoning the mutex lock.
                    return;
                }
            };
            axum::serve(
                listener,
                ws_server_app.into_make_service_with_connect_info::<std::net::SocketAddr>()
            ).await.unwrap();
        });

        // Detached initialization tasks used to leave main_backend waiting forever when an
        // unexpected panic occurred before `initialized` was set. Convert a join failure into a
        // visible module failure and wake every waiter.
        ws_server_runtime.spawn(async move {
            if let Err(error) = initialization_handle.await {
                eprintln!("[{}] [ERROR] initialization task failed: {}", MODULE_IDENTITY, error);
                let mut status = failure_status.lock().await;
                status.panicked = true;
                status.init_notify.notify_waiters();
            }
        });
    }

    {
        let ws_server_status: AsyncModifiable<ModuleStatus> = ws_server_status.clone();
        ws_server_runtime.spawn(async move {
            // Wait for initialization, notified instead of polled. The setter uses
            // notify_waiters() (not notify_one()) because main_backend's own module-loading wait
            // is a second, independent waiter on this same init_notify; notify_waiters() only
            // reaches waiters already registered at the moment it's called, so `enable()` must
            // run here before the flag check to register us immediately.
            let init_notify: std::sync::Arc<tokio::sync::Notify> = {
                ws_server_status.lock().await.init_notify.clone()
            };
            let notified = init_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let already_initialized: bool = ws_server_status.lock().await.initialized;
            if !already_initialized {
                notified.await;
            }
            // Now processing socket message
            crate::ws_server_socket::socket_message_processing().await;
        });
    }

    {
        let ws_server_status: AsyncModifiable<ModuleStatus> = ws_server_status.clone();
        ws_server_runtime.spawn(crate::self_management::self_management(ws_server_status));
    }
    (ws_server_runtime, ws_server_status)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn on_unload(_unload_timeout_ms: usize) {
    println!(
        "{}",
        ansi_term::Color::Purple.paint(
            format!(
                "[{}] [DOWN] [THREAD {}] [FILE `{}` LINE {}] Unloading the Websocket server...",
                MODULE_IDENTITY,
                std::thread::current().id().as_u64(),
                file!(),
                line!()
            )
        )
    );
    println!(
        "{}",
        ansi_term::Color::Purple.paint(
            format!(
                "[{}] [DOWN] [THREAD {}] [FILE `{}` LINE {}] Unloaded the Websocket server.",
                MODULE_IDENTITY,
                std::thread::current().id().as_u64(),
                file!(),
                line!()
            )
        )
    );
}

#[cfg(test)]
mod tests {
    use super::WsServerConfigJson;

    #[test]
    fn bundled_config_deserializes() {
        let config: WsServerConfigJson = serde_json::from_str(include_str!(
            "../ws_server_config_rs.json"
        ))
        .expect("ws_server_config_rs.json must match WsServerConfigJson");

        assert!(!config.external_listeners_list.is_empty());
        assert!(config
            .external_listeners_list
            .iter()
            .all(|listener| !listener.command.is_empty()
                && semver::VersionReq::parse(&listener.required_protocol_version).is_ok()));
    }
}
