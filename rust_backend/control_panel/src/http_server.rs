use crate::global::*;
use axum::response::IntoResponse;
use mysql_async::prelude::*;

#[derive(serde::Serialize)]
struct ApiResponse {
    ok: bool,
    message: String,
}

#[derive(serde::Serialize)]
struct MetricPointJson {
    label: String,
    daily: i64,
    cumulative: i64,
}

#[derive(serde::Serialize)]
struct MetricSeriesJson {
    key: String,
    title: String,
    points: Vec<MetricPointJson>,
}

#[derive(serde::Serialize)]
struct StatsResponse {
    ok: bool,
    metrics: Vec<MetricSeriesJson>,
}

// Builds the router, binds the HTTP listener (walking the port upward on conflict), marks the
// module initialized so main_backend stops waiting on us, then serves forever.
pub async fn serve(control_panel_status: AsyncModifiable<ModuleStatus>) {
    // Make sure the database and visits table exist before we accept requests.
    let schema_result: Result<(), String> = async {
        crate::analytics::ensure_schema().await
    }.await;
    if let Err(error) = schema_result {
        eprintln!(
            "{}",
            ansi_term::Color::Red.paint(
                format!(
                    "[{}] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Failed to initialize the control-panel schema: {}",
                    MODULE_IDENTITY,
                    std::thread::current().id().as_u64(),
                    file!(),
                    line!(),
                    error
                )
            )
        );
        let mut guard_control_panel_status: tokio::sync::MutexGuard<'_, ModuleStatus> =
            control_panel_status.lock().await;
        guard_control_panel_status.panicked = true;
        guard_control_panel_status.init_notify.notify_waiters();
        drop(guard_control_panel_status); // Avoid poisoning the mutex lock.
        return;
    }

    if !control_panel_is_configured() {
        eprintln!(
            "{}",
            ansi_term::Color::Yellow.paint(
                "[CONTROL_PANEL] [WARNING] No control-panel role token is set; admin APIs will return 503."
            )
        );
    }

    // Configure CORS to allow requests from the frontend dev server
    let mut allowed_origins = vec![
        "http://localhost:5173".parse::<axum::http::HeaderValue>().unwrap(),
        "http://127.0.0.1:5173".parse::<axum::http::HeaderValue>().unwrap(),
    ];
    if let Ok(origin) = std::env::var("CONTROL_PANEL_CORS_ORIGIN")
        && let Ok(origin) = origin.parse::<axum::http::HeaderValue>() {
            allowed_origins.push(origin);
        }
    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(allowed_origins)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ]);

    // Keep the visit endpoint public because ws_server calls it before a user is logged in.
    // Every other data/admin endpoint is behind the same bearer-token middleware, including the
    // role lookup endpoint. This prevents a caller from using a valid username as an admin check.
    let protected_routes: axum::Router = axum::Router
        ::new()
        .route("/api/clear-database", axum::routing::post(clear_database_handler))
        .route("/api/stats", axum::routing::post(stats_handler))
        // System monitoring
        .route("/api/system-status", axum::routing::post(system_status_handler))
        .route("/api/database-stats", axum::routing::post(database_stats_handler))
        // Admin verification
        .route("/api/verify-admin", axum::routing::post(verify_admin_handler))
        // User management
        .route("/api/users", axum::routing::post(users_list_handler))
        .route("/api/users/{id}/delete", axum::routing::post(delete_user_handler))
        // Problem management
        .route("/api/problems", axum::routing::post(problems_list_handler))
        .route("/api/problems/{id}", axum::routing::get(get_problem_handler))
        .route("/api/problems/{id}", axum::routing::put(update_problem_handler))
        .route("/api/problems/create", axum::routing::post(create_problem_handler))
        .route("/api/problems/{id}/delete", axum::routing::post(delete_problem_handler))
        .route("/api/upload-testdata", axum::routing::post(upload_testdata_handler))
        .layer(axum::middleware::from_fn(admin_auth_middleware));

    let app: axum::Router = axum::Router
        ::new()
        .route("/", axum::routing::get(index_page))
        .route("/api/status", axum::routing::get(control_panel_status_handler))
        .route("/api/setup", axum::routing::post(control_panel_setup_handler))
        .route("/api/login", axum::routing::post(control_panel_login_handler))
        .route("/api/record-visit", axum::routing::post(record_visit_handler))
        .route("/api/record-ws-connection", axum::routing::post(record_ws_connection_handler))
        .merge(protected_routes)
        .layer(cors);

    let mut http_port: u16 = CONTROL_PANEL_HTTP_PORT;
    let listener: tokio::net::TcpListener = loop {
        match
            tokio::net::TcpListener::bind(format!("{CONTROL_PANEL_HTTP_HOST}:{http_port}")).await
        {
            Ok(listener) => {
                println!(
                    "{}",
                    ansi_term::Color::Green.paint(
                        format!(
                            "[{}] [INFO] [THREAD {}] [FILE `{}` LINE {}] Control panel listening on http://{}:{}",
                            MODULE_IDENTITY,
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!(),
                            CONTROL_PANEL_HTTP_HOST,
                            http_port
                        )
                    )
                );
                break listener;
            }
            Err(_) => {
                println!(
                    "{}",
                    ansi_term::Color::Yellow.paint(
                        format!(
                            "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] Failed to bind the control panel on port {}. Retrying...",
                            MODULE_IDENTITY,
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!(),
                            http_port
                        )
                    )
                );
                if http_port == u16::MAX {
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
                    let mut guard_control_panel_status: tokio::sync::MutexGuard<'_, ModuleStatus> =
                        control_panel_status.lock().await;
                    guard_control_panel_status.panicked = true;
                    guard_control_panel_status.init_notify.notify_waiters();
                    drop(guard_control_panel_status); // Avoid poisoning the mutex lock.
                    return;
                }
                http_port += 1;
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    };

    {
        let mut guard_control_panel_status: tokio::sync::MutexGuard<'_, ModuleStatus> =
            control_panel_status.lock().await;
        *guard_control_panel_status.socket_port.lock().await = http_port;
        guard_control_panel_status.initialized = true;
        // notify_waiters(), not notify_one(): main_backend's module-loading wait and this module's
        // own self_management both block on this same init_notify. notify_one() would wake only one
        // of them and permanently starve the other.
        guard_control_panel_status.init_notify.notify_waiters();
        drop(guard_control_panel_status);
    }

    axum::serve(listener, app.into_make_service()).await.unwrap();
}

async fn index_page() -> axum::response::Html<&'static str> {
    axum::response::Html(crate::panel_html::PANEL_HTML)
}

// The token is deliberately supplied out-of-band through the process environment. It never gets
// generated by an unauthenticated HTTP endpoint and is never stored in the database.
fn configured_role_tokens() -> [(AdminRole, Option<&'static str>); 3] {
    [
        (AdminRole::Super, CONTROL_PANEL_ADMIN_TOKEN.as_deref()),
        (AdminRole::Problem, CONTROL_PANEL_PROBLEM_ADMIN_TOKEN.as_deref()),
        (AdminRole::Community, CONTROL_PANEL_COMMUNITY_ADMIN_TOKEN.as_deref()),
    ]
}

fn control_panel_is_configured() -> bool {
    configured_role_tokens().iter().any(|(_, token)| token.is_some())
}

fn constant_time_token_eq(actual: &str, expected: &str) -> bool {
    let actual = actual.as_bytes();
    let expected = expected.as_bytes();
    let mut diff = actual.len() ^ expected.len();
    for index in 0..actual.len().max(expected.len()) {
        diff |= actual.get(index).copied().unwrap_or(0) as usize ^
            expected.get(index).copied().unwrap_or(0) as usize;
    }
    diff == 0
}

async fn control_panel_status_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "configured": control_panel_is_configured(),
        "dev_mode": false,
    }))
}

#[derive(serde::Deserialize)]
struct AdminTokenRequest {
    token: String,
}

async fn control_panel_setup_handler() -> (axum::http::StatusCode, axum::Json<ApiResponse>) {
    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        axum::Json(ApiResponse {
            ok: false,
            message: String::from(
                "Automatic setup is disabled. Set a control-panel role token and restart the backend.",
            ),
        }),
    )
}

async fn control_panel_login_handler(
    axum::Json(request): axum::Json<AdminTokenRequest>,
) -> (axum::http::StatusCode, axum::Json<ApiResponse>) {
    if role_for_token(&request.token).is_some() {
        (
            axum::http::StatusCode::OK,
            axum::Json(ApiResponse { ok: true, message: String::from("Authenticated") }),
        )
    } else {
        (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(ApiResponse { ok: false, message: String::from("Invalid admin token") }),
        )
    }
}

fn role_for_token(actual: &str) -> Option<AdminRole> {
    configured_role_tokens()
        .into_iter()
        .find_map(|(role, expected)| {
            expected
                .filter(|expected| constant_time_token_eq(actual, expected))
                .map(|_| role)
        })
}

#[derive(Clone, Copy)]
enum RequiredAdminRole {
    Any,
    Problem,
    Super,
}

fn required_role_for_path(path: &str) -> RequiredAdminRole {
    if path == "/api/stats" || path == "/api/verify-admin" {
        RequiredAdminRole::Any
    } else if path.starts_with("/api/problems") || path == "/api/upload-testdata" {
        RequiredAdminRole::Problem
    } else {
        RequiredAdminRole::Super
    }
}

fn role_has_permission(role: AdminRole, required: RequiredAdminRole) -> bool {
    match required {
        RequiredAdminRole::Any => true,
        RequiredAdminRole::Problem => matches!(role, AdminRole::Super | AdminRole::Problem),
        RequiredAdminRole::Super => role == AdminRole::Super,
    }
}

async fn admin_auth_middleware(
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if !control_panel_is_configured() {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({
                "ok": false,
                "message": "Control panel is not configured. Set a control-panel role token."
            })),
        ).into_response();
    }

    let role = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .and_then(role_for_token);

    let Some(role) = role else {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "ok": false, "message": "Invalid admin token" })),
        ).into_response();
    };

    if !role_has_permission(role, required_role_for_path(request.uri().path())) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "ok": false,
                "message": "This control-panel credential does not permit that operation"
            })),
        ).into_response();
    }

    request.extensions_mut().insert(role);
    next.run(request).await
}

async fn clear_database_handler(
    axum::Json(_): axum::Json<serde_json::Value>
) -> (axum::http::StatusCode, axum::Json<ApiResponse>) {

    match crate::db_admin::clear_database().await {
        Ok(table_count) => {
            println!(
                "{}",
                ansi_term::Color::Purple.paint(
                    format!(
                        "[{}] [INFO] [THREAD {}] [FILE `{}` LINE {}] Database cleared via control panel ({} table(s) truncated).",
                        MODULE_IDENTITY,
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!(),
                        table_count
                    )
                )
            );
            (
                axum::http::StatusCode::OK,
                axum::Json(ApiResponse {
                    ok: true,
                    message: format!(
                        "Cleared {table_count} table(s). All data has been wiped; the schema is intact."
                    ),
                }),
            )
        }
        Err(error) => internal_error(error),
    }
}

// Records one site visit. No auth: bound to 127.0.0.1, called by ws_server on the same host when
// a new websocket connection is established. Always answers 200 so the (fire-and-forget) caller
// never blocks on us.
async fn record_visit_handler() -> axum::http::StatusCode {
    if let Err(error) = crate::analytics::record_visit().await {
        eprintln!(
            "{}",
            ansi_term::Color::Red.paint(
                format!(
                    "[{}] [ERROR] [THREAD {}] [FILE `{}` LINE {}] Failed to record a visit: {}",
                    MODULE_IDENTITY,
                    std::thread::current().id().as_u64(),
                    file!(),
                    line!(),
                    error
                )
            )
        );
    }
    axum::http::StatusCode::OK
}

// Updates the live WebSocket connection metric. This endpoint is intentionally kept separate
// from visit recording: a visit is durable analytics data, while the connection count is an
// in-memory metric maintained by system_monitor.
#[derive(serde::Deserialize)]
struct WsConnectionMetricRequest {
    delta: i8,
}

async fn record_ws_connection_handler(
    axum::Json(request): axum::Json<WsConnectionMetricRequest>,
) -> axum::http::StatusCode {
    match request.delta {
        1 => crate::system_monitor::increment_ws_connections(),
        -1 => crate::system_monitor::decrement_ws_connections(),
        _ => return axum::http::StatusCode::BAD_REQUEST,
    }
    axum::http::StatusCode::NO_CONTENT
}

// Returns daily + cumulative counts (last 30 days) for every dashboard metric. Requires the
// admin token.
async fn stats_handler(
    axum::Json(_): axum::Json<serde_json::Value>
) -> (axum::http::StatusCode, axum::Json<StatsResponse>) {

    let metrics: Vec<MetricSeriesJson> = crate::analytics
        ::get_all_stats(30).await
        .into_iter()
        .map(|series| MetricSeriesJson {
            key: series.key,
            title: series.title,
            points: series.points
                .into_iter()
                .map(|point| MetricPointJson {
                    label: point.label,
                    daily: point.daily,
                    cumulative: point.cumulative,
                })
                .collect(),
        })
        .collect();

    (axum::http::StatusCode::OK, axum::Json(StatsResponse { ok: true, metrics }))
}

fn internal_error(error: String) -> (axum::http::StatusCode, axum::Json<ApiResponse>) {
    eprintln!(
        "{}",
        ansi_term::Color::Red.paint(
            format!(
                "[{}] [ERROR] [THREAD {}] [FILE `{}` LINE {}] {}",
                MODULE_IDENTITY,
                std::thread::current().id().as_u64(),
                file!(),
                line!(),
                error
            )
        )
    );
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(ApiResponse { ok: false, message: format!("Internal error: {error}") }),
    )
}

// ============================================================================
// Admin verification handler
// ============================================================================

#[derive(serde::Deserialize)]
struct VerifyAdminRequest {
    username: String,
}

#[derive(serde::Serialize)]
struct VerifyAdminResponse {
    ok: bool,
    is_admin: bool,
    admin_role: Option<String>,
    username: Option<String>,
}

async fn verify_admin_handler(
    axum::extract::Extension(credential_role): axum::extract::Extension<AdminRole>,
    axum::Json(request): axum::Json<VerifyAdminRequest>
) -> (axum::http::StatusCode, axum::Json<VerifyAdminResponse>) {
    let mut conn = match MYSQL_DATABASE_POOL.get_conn().await {
        Ok(c) => c,
        Err(_) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(VerifyAdminResponse {
                    ok: false,
                    is_admin: false,
                    admin_role: None,
                    username: None,
                }),
            );
        }
    };

    // Check user's admin role directly by username
    let admin_role: Option<String> = conn
        .exec_first(
            format!(
                "SELECT admin_role FROM `{DATABASE_NAME}`.`users` WHERE username = :username"
            ),
            params! { "username" => &request.username },
        )
        .await
        .unwrap_or(None);

    drop(conn);

    // A role-scoped credential may only unlock an account assigned the same role. API route
    // authorization is still enforced independently by the middleware above.
    let is_admin = admin_role.as_deref() == Some(credential_role.as_database_value());

    (
        axum::http::StatusCode::OK,
        axum::Json(VerifyAdminResponse {
            ok: true,
            is_admin,
            admin_role: is_admin.then_some(credential_role.as_database_value().to_string()),
            username: Some(request.username),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{role_has_permission, AdminRole, RequiredAdminRole};

    #[test]
    fn role_permissions_are_enforced_server_side() {
        assert!(role_has_permission(AdminRole::Super, RequiredAdminRole::Super));
        assert!(role_has_permission(AdminRole::Problem, RequiredAdminRole::Problem));
        assert!(!role_has_permission(AdminRole::Problem, RequiredAdminRole::Super));
        assert!(!role_has_permission(AdminRole::Community, RequiredAdminRole::Problem));
    }

    #[test]
    fn embedded_panel_describes_environment_token_setup() {
        let html = crate::panel_html::PANEL_HTML;
        assert!(html.contains("CONTROL_PANEL_*_TOKEN"));
        assert!(!html.contains("Generate admin token"));
        assert!(!html.contains("debug build bypasses token"));
    }
}

// ============================================================================
// System monitoring handlers
// ============================================================================

async fn system_status_handler(
    axum::Json(_): axum::Json<serde_json::Value>
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {

    match crate::system_monitor::get_system_status().await {
        Ok(status) => (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({ "ok": true, "data": status })),
        ),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "ok": false, "message": error })),
        ),
    }
}

async fn database_stats_handler(
    axum::Json(_): axum::Json<serde_json::Value>
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {

    match crate::system_monitor::get_database_stats().await {
        Ok(stats) => (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({ "ok": true, "data": stats })),
        ),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "ok": false, "message": error })),
        ),
    }
}

// ============================================================================
// User management handlers
// ============================================================================

#[derive(serde::Deserialize)]
struct UserListRequest {
    page: i64,
    page_size: i64,
    search: Option<String>,
}

async fn users_list_handler(
    axum::Json(request): axum::Json<UserListRequest>
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {

    let query = crate::user_management::UserListQuery {
        page: request.page,
        page_size: request.page_size,
        search: request.search,
    };

    match crate::user_management::get_user_list(query).await {
        Ok(response) => (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({ "ok": true, "data": response })),
        ),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "ok": false, "message": error })),
        ),
    }
}

async fn delete_user_handler(
    axum::extract::Path(user_id): axum::extract::Path<i64>,
    axum::Json(_): axum::Json<serde_json::Value>
) -> (axum::http::StatusCode, axum::Json<ApiResponse>) {

    match crate::user_management::delete_user(user_id).await {
        Ok(()) => (
            axum::http::StatusCode::OK,
            axum::Json(ApiResponse { ok: true, message: String::from("User deleted successfully") }),
        ),
        Err(error) => internal_error(error),
    }
}

// ============================================================================
// Problem management handlers
// ============================================================================

#[derive(serde::Deserialize)]
struct ProblemListRequest {
    page: i64,
    page_size: i64,
    search: Option<String>,
}

async fn problems_list_handler(
    axum::Json(request): axum::Json<ProblemListRequest>
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {

    let query = crate::problem_management::ProblemListQuery {
        page: request.page,
        page_size: request.page_size,
        search: request.search,
    };

    match crate::problem_management::get_problem_list(query).await {
        Ok(response) => (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({ "ok": true, "data": response })),
        ),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "ok": false, "message": error })),
        ),
    }
}

async fn create_problem_handler(
    axum::Json(request): axum::Json<serde_json::Value>
) -> (axum::http::StatusCode, axum::Json<ApiResponse>) {
    // Parse create problem request
    let create_request = match serde_json::from_value::<crate::problem_management::CreateProblemRequest>(request) {
        Ok(req) => req,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(ApiResponse {
                    ok: false,
                    message: format!("Invalid request format: {}", e)
                }),
            );
        }
    };

    match crate::problem_management::create_problem(create_request).await {
        Ok(()) => {
            println!(
                "{}",
                ansi_term::Color::Green.paint(
                    format!(
                        "[{}] [INFO] [THREAD {}] [FILE `{}` LINE {}] Problem created via control panel.",
                        MODULE_IDENTITY,
                        std::thread::current().id().as_u64(),
                        file!(),
                        line!()
                    )
                )
            );
            (
                axum::http::StatusCode::OK,
                axum::Json(ApiResponse { ok: true, message: String::from("Problem created successfully") }),
            )
        }
        Err(error) => internal_error(error),
    }
}

async fn delete_problem_handler(
    axum::extract::Path(problem_id): axum::extract::Path<i64>,
    axum::Json(_): axum::Json<serde_json::Value>
) -> (axum::http::StatusCode, axum::Json<ApiResponse>) {

    match crate::problem_management::delete_problem(problem_id).await {
        Ok(()) => (
            axum::http::StatusCode::OK,
            axum::Json(ApiResponse { ok: true, message: String::from("Problem deleted successfully") }),
        ),
        Err(error) => internal_error(error),
    }
}

async fn get_problem_handler(
    axum::extract::Path(problem_id): axum::extract::Path<i64>,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    match crate::problem_management::get_problem_detail(problem_id).await {
        Ok(problem) => (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({ "ok": true, "problem": problem })),
        ),
        Err(error) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "ok": false, "message": error })),
        ),
    }
}

async fn update_problem_handler(
    axum::extract::Path(problem_id): axum::extract::Path<i64>,
    axum::Json(request): axum::Json<crate::problem_management::UpdateProblemRequest>,
) -> (axum::http::StatusCode, axum::Json<ApiResponse>) {
    match crate::problem_management::update_problem(problem_id, request).await {
        Ok(()) => (
            axum::http::StatusCode::OK,
            axum::Json(ApiResponse { ok: true, message: String::from("Problem updated successfully") }),
        ),
        Err(error) => internal_error(error),
    }
}

async fn upload_testdata_handler(
    mut multipart: axum::extract::Multipart,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    let mut file_data: Option<Vec<u8>> = None;
    let mut filename: Option<String> = None;
    let mut problem_number: Option<String> = None;
    let mut file_type: Option<String> = None;

    // Parse multipart form data
    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().unwrap_or("").to_string();

        match field_name.as_str() {
            "file" => {
                filename = field.file_name().map(|s| s.to_string());
                file_data = field.bytes().await.ok().map(|b| b.to_vec());
            }
            "problem_number" => {
                problem_number = field.text().await.ok();
            }
            "type" => {
                file_type = field.text().await.ok();
            }
            _ => {}
        }
    }

    // Validate inputs
    let file_data = match file_data {
        Some(data) => data,
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "ok": false, "message": "No file uploaded" })),
            );
        }
    };

    let problem_number = match problem_number {
        Some(n) => n,
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "ok": false, "message": "Missing problem_number" })),
            );
        }
    };

    let file_type = match file_type {
        Some(t) => t,
        None => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                axum::Json(serde_json::json!({ "ok": false, "message": "Missing type" })),
            );
        }
    };

    // The judge reads test data from `<project_root>/problem/<num>/input/` and
    // `.../answer/`, referencing files by their bare name in problem_testcase_config.json.
    // Modules run from `rust_backend/`, so the problem dir is one level up.
    // Map the upload `type` (input/output) onto the judge's dir names (input/answer).
    let subdir = if file_type == "output" || file_type == "answer" {
        "answer"
    } else {
        "input"
    };

    // Preserve the original filename so it matches what the config references.
    let stored_filename = filename
        .as_deref()
        .and_then(|f| f.rsplit(['/', '\\']).next())
        .filter(|s| !s.is_empty())
        .unwrap_or("data.txt")
        .to_string();

    let target_dir = format!("../problem/{}/{}", problem_number, subdir);
    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "ok": false, "message": format!("Failed to create directory: {}", e) })),
        );
    }

    let file_path = format!("{}/{}", target_dir, stored_filename);
    if let Err(e) = std::fs::write(&file_path, file_data) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "ok": false, "message": format!("Failed to write file: {}", e) })),
        );
    }

    (
        axum::http::StatusCode::OK,
        axum::Json(serde_json::json!({ "ok": true, "filename": stored_filename })),
    )
}
