use tokio::io::AsyncWriteExt;

pub static MODULE_IDENTITY: &str = "JUDGE";

pub const SUBMISSIONS_LIST_PAGE_SIZE: i64 = 20;

pub const SOLUTIONS_LIST_PAGE_SIZE: i64 = 10;
pub const SOLUTION_COMMENTS_LIST_PAGE_SIZE: i64 = 8;

pub const DISCUSSIONS_LIST_PAGE_SIZE: i64 = 10;
pub const DISCUSSION_REPLIES_LIST_PAGE_SIZE: i64 = 20;

pub const NOTIFICATIONS_LIST_PAGE_SIZE: i64 = 10;
// Cap on usernames returned by the @mention autocomplete prefix search.
pub const USER_SEARCH_LIMIT: i64 = 8;

pub type AsyncModifiable<T> = std::sync::Arc<tokio::sync::Mutex<T>>;

pub fn new_async_modifiable<T>(x: T) -> AsyncModifiable<T> {
    std::sync::Arc::new(tokio::sync::Mutex::new(x))
}

#[derive(Debug, Clone)]
pub struct ModuleStatus {
    pub initialized: bool,
    pub panicked: bool,
    pub socket_port: AsyncModifiable<u16>,
    pub init_notify: std::sync::Arc<tokio::sync::Notify>,
}

pub static GLOBAL_MODULE_STATUSES_BY_PROTOCOL: std::sync::OnceLock<
    AsyncModifiable<std::collections::HashMap<String, AsyncModifiable<ModuleStatus>>>
> = std::sync::OnceLock::new();

pub static JUDGE_SOCKET: std::sync::OnceLock<AsyncModifiable<tokio::net::TcpListener>> = std::sync::OnceLock::new();

// A submission owns compiler/runtime processes and several megabytes of source and output data.
// Reject new work while all slots are occupied instead of allowing unbounded Tokio tasks to
// exhaust CPU, memory, or process table entries.
pub static JUDGE_CONCURRENCY: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| {
        let permits = std::env::var("RSOJ_JUDGE_CONCURRENCY")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(2);
        std::sync::Arc::new(tokio::sync::Semaphore::new(permits))
    });

pub fn expire_pending<T: Send + 'static>(
    map: &'static tokio::sync::Mutex<std::collections::HashMap<String, T>>,
    request_key: String,
) {
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        map.lock().await.remove(&request_key);
    });
}

// The `problem` directory lives at the project root, one level above `rust_backend` (where every
// module's process actually runs from), mirroring `main_backend`'s `get_parent_path()` convention.
pub fn get_problem_dir_path() -> String {
    let mut pwd: std::path::PathBuf = std::env::current_dir().unwrap();
    pwd.pop();
    pwd.push("problem");
    String::from(pwd.to_str().unwrap())
}

pub const MYSQL_DATABASE_URL: &str = "mysql://root:123456@127.0.0.1:3306/";
pub const DATABASE_NAME: &str = "RsOJ";

pub static MYSQL_DATABASE_POOL: std::sync::LazyLock<mysql_async::Pool> =
    std::sync::LazyLock::new(|| mysql_async::Pool::new(MYSQL_DATABASE_URL));

pub static MODULE_RUNTIME_HANDLE: std::sync::OnceLock<tokio::runtime::Handle> =
    std::sync::OnceLock::new();

pub fn run_shutdown_task<F>(task: F, timeout: std::time::Duration) -> bool
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Some(handle) = MODULE_RUNTIME_HANDLE.get() {
        let (complete_tx, complete_rx) = std::sync::mpsc::channel();
        drop(handle.spawn(async move {
            task.await;
            let _ = complete_tx.send(());
        }));
        complete_rx.recv_timeout(timeout).is_ok()
    } else {
        eprintln!("[{}] [WARNING] Runtime handle is unavailable during shutdown", MODULE_IDENTITY);
        false
    }
}

pub async fn disconnect_database_pool() {
    if let Err(error) = MYSQL_DATABASE_POOL.clone().disconnect().await {
        eprintln!(
            "[{}] [WARNING] Failed to disconnect the database pool during shutdown: {}",
            MODULE_IDENTITY, error
        );
    }
}

pub async fn get_db_conn() -> Result<mysql_async::Conn, mysql_async::Error> {
    MYSQL_DATABASE_POOL.get_conn().await
}

// In-memory map of pending `submissions_list` requests, keyed by the request_key used when
// asking simple_authenticator for the requester's username (resolved from their ws_id). Needed
// because that lookup is an async round trip — when the result comes back, we need to recall
// who asked and which page they wanted.
pub struct PendingSubmissionsListRequest {
    pub requester_ws_id: String,
    pub page_index: i64,
    pub username_filter: String,
    pub original_request_key: String,
}

pub static PENDING_SUBMISSIONS_LIST_REQUESTS: std::sync::LazyLock<
    tokio::sync::Mutex<std::collections::HashMap<String, PendingSubmissionsListRequest>>
> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

// In-memory map of pending `submission` requests, keyed by the request_key used when asking
// simple_authenticator to validate the requester's session. Needed because that lookup is an
// async round trip — when the result comes back, we need to recall the original submission.
pub struct PendingSubmissionRequest {
    pub requester_ws_id: String,
    pub username: String,
    pub problem_number: i64,
    pub language: String,
    pub code: Vec<String>,
    pub is_test_submission_mode: bool,
    pub original_request_key: String,
}

pub static PENDING_SUBMISSION_REQUESTS: std::sync::LazyLock<
    tokio::sync::Mutex<std::collections::HashMap<String, PendingSubmissionRequest>>
> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

// In-memory map of pending `submission_result` (fetch-by-id) requests, keyed by the request_key
// used when asking simple_authenticator to resolve the requester's username from their ws_id.
// Needed to decide whether the requester is the submission's own author (who alone may see
// `code`) before replying.
pub struct PendingSubmissionResultFetchRequest {
    pub requester_ws_id: String,
    pub submission_id: i64,
    pub owner_username: String,
    pub problem_number: i64,
    pub result: String,
    pub general_score: i32,
    pub statuses: Vec<String>,
    pub scores: Vec<i32>,
    pub code: Vec<String>,
    pub language: String,
    pub original_request_key: String,
}

pub static PENDING_SUBMISSION_RESULT_FETCH_REQUESTS: std::sync::LazyLock<
    tokio::sync::Mutex<std::collections::HashMap<String, PendingSubmissionResultFetchRequest>>
> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

// Session-validated ("authed") solution-area actions waiting on the validate-session round trip to
// simple_authenticator. Like submissions, judge can't trust the client's claimed identity, so every
// write (post a solution/comment, cast a vote) is parked here keyed by the validate-session
// request_key until the session is confirmed, then dispatched by its `action` variant.
pub enum PendingSolutionAuthedAction {
    PostSolution {
        problem_number: i64,
        title: String,
        content: Vec<String>,
        is_official: bool,
    },
    VoteSolution {
        solution_id: i64,
        // 1 = like, -1 = dislike, 0 = clear the requester's existing vote.
        vote: i8,
    },
    PostComment {
        solution_id: i64,
        content: Vec<String>,
    },
    VoteComment {
        comment_id: i64,
        vote: i8,
    },
}

pub struct PendingSolutionAuthedRequest {
    pub requester_ws_id: String,
    pub username: String,
    pub original_request_key: String,
    pub action: PendingSolutionAuthedAction,
}

pub static PENDING_SOLUTION_AUTHED_REQUESTS: std::sync::LazyLock<
    tokio::sync::Mutex<std::collections::HashMap<String, PendingSolutionAuthedRequest>>
> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

// Read-side solution-area actions that need the requester's username before they can answer (to
// flag the requester's own vote / ownership), resolved the same way submissions_list does — by
// asking simple_authenticator for the username behind a ws_id. Parked here keyed by that lookup's
// request_key until the result returns.
pub enum PendingSolutionLookup {
    SolutionsList {
        problem_number: i64,
        page_index: i64,
        sort_by_likes: bool,
    },
    SolutionFetch {
        solution_id: i64,
    },
    SolutionCommentsList {
        solution_id: i64,
        page_index: i64,
        sort_by_likes: bool,
    },
}

pub struct PendingSolutionLookupRequest {
    pub requester_ws_id: String,
    pub original_request_key: String,
    pub lookup: PendingSolutionLookup,
}

pub static PENDING_SOLUTION_LOOKUP_REQUESTS: std::sync::LazyLock<
    tokio::sync::Mutex<std::collections::HashMap<String, PendingSolutionLookupRequest>>
> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

// Discussion forum — same two-map pattern as solutions. Session-validated ("authed") writes
// (post a thread/reply, cast a vote) are parked here keyed by the validate-session request_key
// until the session is confirmed, then dispatched by their `action` variant.
pub enum PendingDiscussionAuthedAction {
    PostDiscussion {
        title: String,
        content: Vec<String>,
    },
    VoteDiscussion {
        discussion_id: i64,
        vote: i8,
    },
    PostReply {
        discussion_id: i64,
        content: Vec<String>,
    },
    VoteReply {
        reply_id: i64,
        vote: i8,
    },
}

pub struct PendingDiscussionAuthedRequest {
    pub requester_ws_id: String,
    pub username: String,
    pub original_request_key: String,
    pub action: PendingDiscussionAuthedAction,
}

pub static PENDING_DISCUSSION_AUTHED_REQUESTS: std::sync::LazyLock<
    tokio::sync::Mutex<std::collections::HashMap<String, PendingDiscussionAuthedRequest>>
> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

// Read-side discussion actions that need the requester's username before answering (to flag the
// requester's own vote), resolved by asking simple_authenticator for the username behind a ws_id.
pub enum PendingDiscussionLookup {
    DiscussionsList {
        page_index: i64,
        sort_by_likes: bool,
    },
    DiscussionFetch {
        discussion_id: i64,
    },
    DiscussionRepliesList {
        discussion_id: i64,
        page_index: i64,
        sort_by_likes: bool,
    },
}

pub struct PendingDiscussionLookupRequest {
    pub requester_ws_id: String,
    pub original_request_key: String,
    pub lookup: PendingDiscussionLookup,
}

pub static PENDING_DISCUSSION_LOOKUP_REQUESTS: std::sync::LazyLock<
    tokio::sync::Mutex<std::collections::HashMap<String, PendingDiscussionLookupRequest>>
> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

// Info-center (notifications) — same two-map pattern. Read-side actions need the requester's
// username before answering (the inbox is private to its owner), resolved by asking
// simple_authenticator for the username behind a ws_id. Parked here keyed by that lookup's
// request_key until the result returns.
pub enum PendingNotificationLookup {
    NotificationsList {
        page_index: i64,
    },
    TotalNotificationsListIndex,
    UnreadCount,
}

pub struct PendingNotificationLookupRequest {
    pub requester_ws_id: String,
    pub original_request_key: String,
    pub lookup: PendingNotificationLookup,
}

pub static PENDING_NOTIFICATION_LOOKUP_REQUESTS: std::sync::LazyLock<
    tokio::sync::Mutex<std::collections::HashMap<String, PendingNotificationLookupRequest>>
> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

// Session-validated ("authed") info-center writes (mark a notification — or all of them — read).
// Parked here keyed by the validate-session request_key until the session is confirmed, so a user
// can only ever mutate their own inbox.
pub enum PendingNotificationAuthedAction {
    MarkRead {
        // 0 marks every unread notification for the requester; otherwise the single id.
        notification_id: i64,
    },
}

pub struct PendingNotificationAuthedRequest {
    pub requester_ws_id: String,
    pub username: String,
    pub original_request_key: String,
    pub action: PendingNotificationAuthedAction,
}

pub static PENDING_NOTIFICATION_AUTHED_REQUESTS: std::sync::LazyLock<
    tokio::sync::Mutex<std::collections::HashMap<String, PendingNotificationAuthedRequest>>
> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

#[derive(serde::Deserialize, serde::Serialize, std::fmt::Debug)]
pub struct SocketJsonMessage {
    pub r#type: String,
    pub content: serde_json::Value,
    pub request_key: String,
    pub from_protocol: String,
}

#[derive(serde::Deserialize, serde::Serialize, std::fmt::Debug)]
pub struct SocketJsonMessageWithWsId {
    pub r#type: String,
    pub content: serde_json::Value,
    pub request_key: String,
    pub from_protocol: String,
    pub ws_id: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SocketJsonMessageContentOnSendMsg {
    pub ws_id: String,
    pub msg_to_send: serde_json::Value,
}

pub async fn get_socket_port_by_protocol(protocol: &str) -> Option<u16> {
    let guard_global_module_statuses_by_protocol: tokio::sync::MutexGuard<
        '_,
        std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<ModuleStatus>>>
    > = GLOBAL_MODULE_STATUSES_BY_PROTOCOL.get()?.lock().await;
    let guard_status: tokio::sync::MutexGuard<
        '_,
        ModuleStatus
    > = guard_global_module_statuses_by_protocol.get(protocol)?.lock().await;
    Some(*guard_status.socket_port.lock().await)
}

pub async fn get_socket_by_protocol(protocol: &str) -> tokio::net::TcpStream {
    let socket_port: u16 = match get_socket_port_by_protocol(protocol).await {
        Some(x) => x,
        None => {
            eprintln!(
                "{}",
                ansi_term::Color::Red.paint(
                    format!(
                        "[{}] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Failed to get the port of the module implemented protocol `{}`.",
                        MODULE_IDENTITY,
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!(),
                        protocol
                    )
                )
            );
            panic!();
        }
    };
    match tokio::net::TcpStream::connect(format!("127.0.0.1:{socket_port}")).await {
        Ok(socket) => {
            socket.set_nodelay(true).ok();
            socket
        }
        Err(_) => {
            eprintln!(
                "{}",
                ansi_term::Color::Red.paint(
                    format!(
                        "[{}] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Failed to connect to the socket of the module implemented protocol `{}` on port {}.",
                        MODULE_IDENTITY,
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!(),
                        protocol,
                        socket_port
                    )
                )
            );
            panic!();
        }
    }
}

pub async fn send_socket_json_message(msg_to_send: &serde_json::Value, to_protocol: &str) {
    let mut socket = get_socket_by_protocol(to_protocol).await;
    let json_msg_str = serde_json::to_string(&msg_to_send).unwrap();
    let result = socket.write_all(json_msg_str.as_bytes()).await;
    if result.is_err() {
        println!(
            "{}",
            ansi_term::Color::Yellow.paint(
                format!(
                    "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] Failed to send message to protocol `{}`.",
                    MODULE_IDENTITY,
                    std::thread::current().id().as_u64(),
                    to_protocol,
                    file!(),
                    line!()
                )
            )
        );
    }
}
