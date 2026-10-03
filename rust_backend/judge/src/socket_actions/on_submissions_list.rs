use crate::global::*;

#[derive(serde::Deserialize, serde::Serialize)]
struct SocketJsonMessageContentOnSubmissionsList {
    index: i64,
    #[serde(default)]
    username: String,
    request_key: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ContentOnUsernameByWsId {
    ws_id: String,
    request_key: String,
}

pub async fn on_submissions_list(msg: SocketJsonMessageWithWsId) {
    if
        let Ok(content) = serde_json::from_value::<SocketJsonMessageContentOnSubmissionsList>(
            msg.content
        )
    {
        let lookup_request_key: String = uuid::Uuid::new_v4().to_string();

        let mut guard_pending_submissions_list_requests: tokio::sync::MutexGuard<
            '_,
            std::collections::HashMap<String, PendingSubmissionsListRequest>
        > = PENDING_SUBMISSIONS_LIST_REQUESTS.lock().await;
        guard_pending_submissions_list_requests.insert(
            lookup_request_key.clone(),
            PendingSubmissionsListRequest {
                requester_ws_id: msg.ws_id.clone(),
                page_index: content.index,
                username_filter: content.username.trim().to_string(),
                original_request_key: content.request_key,
            }
        );
        drop(guard_pending_submissions_list_requests);
        expire_pending(&*PENDING_SUBMISSIONS_LIST_REQUESTS, lookup_request_key.clone());

        // judge has no notion of usernames itself — only simple_authenticator tracks which
        // username owns a given ws_id — so ask it directly (point-to-point module-to-module
        // communication) rather than duplicating that state here.
        let msg_to_send: SocketJsonMessage = SocketJsonMessage {
            r#type: String::from("on_username_by_ws_id"),
            content: serde_json
                ::to_value(ContentOnUsernameByWsId {
                    ws_id: msg.ws_id,
                    request_key: lookup_request_key.clone(),
                })
                .unwrap(),
            request_key: lookup_request_key,
            from_protocol: String::from("std_judge"),
        };
        send_socket_json_message(
            &serde_json::to_value(msg_to_send).unwrap(),
            "std_authenticator"
        ).await;
    } else {
        println!(
            "{}",
            ansi_term::Color::Yellow.paint(
                format!(
                    "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] The JSON message received is in wrong format.",
                    MODULE_IDENTITY,
                    std::thread::current().id().as_u64(),
                    file!(),
                    line!()
                )
            )
        );
    }
}
