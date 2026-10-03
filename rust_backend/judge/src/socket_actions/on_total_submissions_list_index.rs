use crate::global::*;

use mysql_async::prelude::*;

#[derive(serde::Deserialize, serde::Serialize)]
struct SocketJsonMessageContentOnTotalSubmissionsListIndex {
    #[serde(default)]
    username: String,
    request_key: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ContentInTotalSubmissionsListIndexResult {
    total_submissions_list_index: i64,
    request_key: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct TotalSubmissionsListIndexResult {
    r#type: String,
    content: ContentInTotalSubmissionsListIndexResult,
}

pub async fn on_total_submissions_list_index(msg: SocketJsonMessageWithWsId) {
    if
        let Ok(content) = serde_json::from_value::<
            SocketJsonMessageContentOnTotalSubmissionsListIndex
        >(msg.content)
    {
        let mut conn: mysql_async::Conn = get_db_conn().await.unwrap();
        let results: Result<Vec<i64>, _> = if content.username.trim().is_empty() {
            conn.query("SELECT COUNT(*) FROM RsOJ.submissions").await
        } else {
            conn.exec(
                "SELECT COUNT(*) FROM RsOJ.submissions WHERE username = :username",
                mysql_async::params! { "username" => content.username.trim() },
            ).await
        };
        drop(conn);

        match results {
            Ok(counts) => {
                let total_submissions: i64 = counts.into_iter().next().unwrap_or(0);
                // At least one page even when there are no submissions yet, since the frontend
                // displays "page X / max(total, 1)" and never lets the index drop below 1.
                let total_submissions_list_index: i64 = std::cmp::max(
                    1,
                    (total_submissions + SUBMISSIONS_LIST_PAGE_SIZE - 1) / SUBMISSIONS_LIST_PAGE_SIZE
                );

                let total_submissions_list_index_result = TotalSubmissionsListIndexResult {
                    r#type: String::from("total_submissions_list_index"),
                    content: ContentInTotalSubmissionsListIndexResult {
                        total_submissions_list_index,
                        request_key: content.request_key,
                    },
                };

                let msg_to_send: SocketJsonMessage = SocketJsonMessage {
                    r#type: String::from("on_send_msg"),
                    content: serde_json
                        ::to_value(SocketJsonMessageContentOnSendMsg {
                            ws_id: msg.ws_id,
                            msg_to_send: serde_json
                                ::to_value(total_submissions_list_index_result)
                                .unwrap(),
                        })
                        .unwrap(),
                    request_key: msg.request_key,
                    from_protocol: String::from("std_judge"),
                };
                send_socket_json_message(
                    &serde_json::to_value(msg_to_send).unwrap(),
                    "std_ws_server"
                ).await;
            }
            Err(e) => {
                println!(
                    "{}",
                    ansi_term::Color::Yellow.paint(
                        format!(
                            "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] Failed to count the total submissions (The SQL query is not correct).",
                            MODULE_IDENTITY,
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!()
                        )
                    )
                );
                println!(
                    "{}",
                    ansi_term::Color::Yellow.paint(
                        format!(
                            "[{}] [WARNING] [THREAD {}] [FILE `{}` LINE {}] {}",
                            MODULE_IDENTITY,
                            std::thread::current().id().as_u64(),
                            file!(),
                            line!(),
                            e
                        )
                    )
                );
            }
        }
    }
}
