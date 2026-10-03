#![feature(thread_id_value)]

mod global;
mod socket_actions;
mod self_management;
mod socket_message_processing;
mod simple_authenticator_socket;

use mysql_async::prelude::{ Query, Queryable };

use crate::global::*;

#[unsafe(no_mangle)]
pub extern "Rust" fn on_init(
    global_module_statuses_by_protocol: AsyncModifiable<
        std::collections::HashMap<String, AsyncModifiable<ModuleStatus>>
    >
) -> (tokio::runtime::Runtime, AsyncModifiable<ModuleStatus>) {
    let simple_authenticator_runtime: tokio::runtime::Runtime = tokio::runtime::Builder
        ::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    MODULE_RUNTIME_HANDLE.set(simple_authenticator_runtime.handle().clone()).unwrap();
    GLOBAL_MODULE_STATUSES_BY_PROTOCOL.set(global_module_statuses_by_protocol.clone()).unwrap();
    let simple_authenticator_status: ModuleStatus = ModuleStatus {
        initialized: false,
        panicked: false,
        socket_port: new_async_modifiable(0),
        init_notify: std::sync::Arc::new(tokio::sync::Notify::new()),
    };
    let simple_authenticator_status: AsyncModifiable<ModuleStatus> = new_async_modifiable(
        simple_authenticator_status
    );
    {
        let initialization_status: AsyncModifiable<ModuleStatus> = simple_authenticator_status.clone();
        let initialization_handle = simple_authenticator_runtime.spawn(async move {
            let simple_authenticator_status = initialization_status;
            let mut conn: mysql_async::Conn = match MYSQL_DATABASE_POOL.get_conn().await {
                Ok(conn) => conn,
                Err(error) => {
                    eprintln!("[{}] [ERROR] database initialization failed: {}", MODULE_IDENTITY, error);
                    let mut status = simple_authenticator_status.lock().await;
                    status.panicked = true;
                    status.init_notify.notify_waiters();
                    return;
                }
            };
            let tmp: Vec<String> = conn.query("SHOW DATABASES LIKE \'RsOJ\'").await.unwrap();
            if !tmp.iter().any(|x| x == "RsOJ") {
                "CREATE DATABASE RsOJ".ignore(&mut conn).await.unwrap();
            }
            "USE RsOJ".ignore(&mut conn).await.unwrap();
            let tmp: Vec<String> = conn.query("SHOW TABLES LIKE \'users\'").await.unwrap();
            if !tmp.iter().any(|x| x == "users") {
                "CREATE TABLE users (
                    id INT AUTO_INCREMENT PRIMARY KEY NOT NULL,
                    username VARCHAR(256) NOT NULL,
                    password VARCHAR(256) NOT NULL,
                    accepted INT NOT NULL DEFAULT 0,
                    test_accepted INT NOT NULL DEFAULT 0,
                    general INT NOT NULL DEFAULT 0,
                    created_at BIGINT NOT NULL DEFAULT 0
                )"
                    .ignore(&mut conn).await
                    .unwrap();
            }
            for column in ["accepted", "test_accepted", "general"] {
                let tmp: Vec<String> = conn
                    .query(
                        format!(
                            "SELECT column_name FROM information_schema.columns WHERE table_schema = 'RsOJ' AND table_name = 'users' AND column_name = '{column}'"
                        )
                    )
                    .await
                    .unwrap();
                if tmp.is_empty() {
                    format!("ALTER TABLE users ADD COLUMN {column} INT NOT NULL DEFAULT 0")
                        .ignore(&mut conn).await
                        .unwrap();
                }
            }
            let admin_role_column: Vec<String> = conn
                .query(
                    "SELECT column_name FROM information_schema.columns WHERE table_schema = 'RsOJ' AND table_name = 'users' AND column_name = 'admin_role'"
                )
                .await
                .unwrap();
            if admin_role_column.is_empty() {
                "ALTER TABLE users ADD COLUMN admin_role VARCHAR(32) NULL"
                    .ignore(&mut conn).await
                    .unwrap();
            }
            // created_at is BIGINT (epoch millis), added later than the columns above. Existing
            // rows backfill to 0, so pre-existing users fall before any 30-day window and count
            // only toward the cumulative baseline, never a daily spike.
            {
                let tmp: Vec<String> = conn
                    .query(
                        "SELECT column_name FROM information_schema.columns WHERE table_schema = 'RsOJ' AND table_name = 'users' AND column_name = 'created_at'"
                    )
                    .await
                    .unwrap();
                if tmp.is_empty() {
                    "ALTER TABLE users ADD COLUMN created_at BIGINT NOT NULL DEFAULT 0"
                        .ignore(&mut conn).await
                        .unwrap();
                }
            }
            "CREATE TABLE IF NOT EXISTS follows (
                follower_username VARCHAR(256) NOT NULL,
                followee_username VARCHAR(256) NOT NULL,
                PRIMARY KEY (follower_username, followee_username)
            )"
                .ignore(&mut conn).await
                .unwrap();
            // Login, registration and username lookups all use username as their key.
            let _ = "ALTER TABLE users ADD UNIQUE INDEX uq_users_username (username)"
                .ignore(&mut conn).await;
            let _ = "ALTER TABLE follows ADD INDEX idx_follows_followee (followee_username)"
                .ignore(&mut conn).await;
            drop(conn);
            let mut guard_simple_authenticator_status: tokio::sync::MutexGuard<
                '_,
                ModuleStatus
            > = simple_authenticator_status.lock().await; // Get the status of the server.
            // Try to establish a socket for messaging.
            let mut simple_authenticator_socket_port: u16 = 9000;
            let mut guard_simple_authenticator_status_socket_port: tokio::sync::MutexGuard<
                '_,
                u16
            > = guard_simple_authenticator_status.socket_port.lock().await;
            let simple_authenticator_socket: tokio::net::TcpListener;
            (simple_authenticator_socket, *guard_simple_authenticator_status_socket_port) = loop {
                let simple_authenticator_socket_result: Result<
                    tokio::net::TcpListener,
                    std::io::Error
                > = tokio::net::TcpListener::bind(
                    format!("127.0.0.1:{}", simple_authenticator_socket_port)
                ).await;
                if let Ok(x) = simple_authenticator_socket_result {
                    println!(
                        "{}",
                        ansi_term::Color::Green.paint(
                            format!(
                                "[{}] [INFO] [THREAD {}] [FILE `{}` LINE {}] Initialized the socket on port {}.",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!(),
                                simple_authenticator_socket_port
                            )
                        )
                    );
                    break (x, simple_authenticator_socket_port);
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
                                simple_authenticator_socket_port
                            )
                        )
                    );
                }
                if simple_authenticator_socket_port == u16::MAX {
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
                simple_authenticator_socket_port += 1;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            };
            SIMPLE_AUTHENTICATOR_SOCKET.set(
                new_async_modifiable(simple_authenticator_socket)
            ).unwrap();
            drop(guard_simple_authenticator_status_socket_port);
            guard_simple_authenticator_status.initialized = true;
            // notify_waiters(), not notify_one(): there are two independent waiters on this same
            // init_notify — main_backend's module-loading wait, and this module's own internal
            // wait below before it starts processing its socket. notify_one() only wakes one of
            // them, permanently starving the other.
            guard_simple_authenticator_status.init_notify.notify_waiters();
            drop(guard_simple_authenticator_status);
        });

        // A panic in database/schema initialization used to leave the main loader waiting
        // forever. Convert the detached task failure into an explicit module failure.
        let failure_status = simple_authenticator_status.clone();
        simple_authenticator_runtime.spawn(async move {
            if let Err(error) = initialization_handle.await {
                eprintln!(
                    "[{}] [ERROR] initialization task failed: {}",
                    MODULE_IDENTITY,
                    error
                );
                let mut status = failure_status.lock().await;
                status.panicked = true;
                status.init_notify.notify_waiters();
            }
        });
    }

    {
        let simple_authenticator_status: AsyncModifiable<ModuleStatus> =
            simple_authenticator_status.clone();
        simple_authenticator_runtime.spawn(async move {
            // Wait for initialization, notified instead of polled. The setter uses
            // notify_waiters() (not notify_one()) because main_backend's own module-loading wait
            // is a second, independent waiter on this same init_notify; notify_waiters() only
            // reaches waiters already registered at the moment it's called, so `enable()` must
            // run here before the flag check to register us immediately.
            let init_notify: std::sync::Arc<tokio::sync::Notify> = simple_authenticator_status
                .lock().await.init_notify
                .clone();
            let notified = init_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let already_initialized: bool = simple_authenticator_status.lock().await.initialized;
            if !already_initialized {
                notified.await;
            }

            // Now processing socket message
            crate::socket_message_processing::socket_message_processing().await;
        });
    }

    {
        let initialization_status = simple_authenticator_status.clone();
        simple_authenticator_runtime.spawn(async move {
            // Do not start looking for ws_server until this module's own initialization has
            // completed. If database setup failed, the supervisor marks the module as panicked
            // and this task exits instead of printing a misleading wait message forever.
            let init_notify = initialization_status.lock().await.init_notify.clone();
            let notified = init_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let already_initialized = initialization_status.lock().await.initialized;
            if !already_initialized {
                notified.await;
            }
            if initialization_status.lock().await.panicked {
                return;
            }

            // ws_server doesn't necessarily exist in the map yet (it may not have been loaded by
            // main_backend at all), so poll until it registers itself. Log this only once because
            // Tokio may resume the same task on different worker threads.
            let mut waiting_logged = false;
            let ws_server_status: AsyncModifiable<ModuleStatus> = loop {
                let guard_global_module_statuses_by_protocol =
                    global_module_statuses_by_protocol.lock().await;
                if
                    let Some(ws_server_status) =
                        guard_global_module_statuses_by_protocol.get("std_ws_server")
                {
                    let ws_server_status: AsyncModifiable<ModuleStatus> = ws_server_status.clone();
                    drop(guard_global_module_statuses_by_protocol);
                    break ws_server_status;
                }
                drop(guard_global_module_statuses_by_protocol);
                if !waiting_logged {
                    println!(
                        "{}",
                        ansi_term::Color::Blue.paint(
                            format!(
                                "[{}] [INFO] [THREAD {}] [FILE `{}` LINE {}] Waiting for the Websocket server to be initialized...",
                                MODULE_IDENTITY,
                                std::thread::current().id().as_u64(),
                                file!(),
                                line!()
                            )
                        )
                    );
                    waiting_logged = true;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            };
            // Now that ws_server is registered, wait for it to finish initializing — notified
            // instead of polled, same as init_notify everywhere else. In practice ws_server only
            // appears in the map after main_backend's own wait for it already completed, so
            // `already_initialized` will already be true here; `enable()` is kept for
            // consistency with the other waiters in case that ordering ever changes.
            let init_notify: std::sync::Arc<tokio::sync::Notify> = ws_server_status
                .lock().await.init_notify
                .clone();
            let notified = init_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let already_initialized: bool = ws_server_status.lock().await.initialized;
            if !already_initialized {
                notified.await;
            }
            println!(
                "{}",
                ansi_term::Color::Green.paint(
                    format!(
                        "[{}] [INFO] [THREAD {}] [FILE `{}` LINE {}] The Websocket server has been initialized.",
                        MODULE_IDENTITY,
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!()
                    )
                )
            );

            simple_authenticator_socket::connect_to_ws_server().await;
        });
    }

    {
        let simple_authenticator_status: AsyncModifiable<ModuleStatus> =
            simple_authenticator_status.clone();
        simple_authenticator_runtime.spawn(
            self_management::self_management(simple_authenticator_status)
        );
    }
    (simple_authenticator_runtime, simple_authenticator_status)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn on_unload(unload_timeout_ms: usize) {
    println!(
        "{}",
        ansi_term::Color::Purple.paint(
            format!(
                "[{}] [DOWN] [THREAD {}] [FILE `{}` LINE {}] Unloading the simple authenticator...",
                MODULE_IDENTITY,
                std::thread::current().id().as_u64(),
                file!(),
                line!()
            )
        )
    );

    let cleanup_completed = run_shutdown_task(async move {
        // Resolve whether ws_server is up and release both locks before calling
        // disconnect_from_ws_server() below, since it locks GLOBAL_MODULE_STATUSES_BY_PROTOCOL
        // itself again internally (via get_socket_port_by_protocol) — holding it here too would
        // self-deadlock the task on tokio::sync::Mutex, which isn't reentrant.
        let ws_server_initialized: bool = {
            let guard_global_module_statuses_by_protocol = GLOBAL_MODULE_STATUSES_BY_PROTOCOL.get()
                .unwrap()
                .lock().await;
            match guard_global_module_statuses_by_protocol.get("std_ws_server") {
                Some(ws_server_status) => ws_server_status.lock().await.initialized,
                None => false,
            }
        };
        if ws_server_initialized {
            simple_authenticator_socket::disconnect_from_ws_server().await;
        }
        disconnect_database_pool().await;
    }, std::time::Duration::from_millis(unload_timeout_ms as u64));

    println!(
        "{}",
        ansi_term::Color::Purple.paint(
            format!(
                "[{}] [DOWN] [THREAD {}] [FILE `{}` LINE {}] Simple authenticator shutdown cleanup {}.",
                MODULE_IDENTITY,
                std::thread::current().id().as_u64(),
                file!(),
                line!(),
                if cleanup_completed { "completed" } else { "timed out" }
            )
        )
    );
}
