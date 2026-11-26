use std::time::Duration;

use anyhow::{Context, Result, ensure};
use reqwest::{Client, Url};
use sea_orm::ActiveValue::Set;
use sea_orm::sea_query::Expr;
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect,
};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tracing::{error, info, warn};
use uuid::Uuid;
use warpgate_db_entities::{Target, TicketRequest, TicketRequestNotification as Outbox, User};

#[derive(Clone, Copy)]
enum Destination {
    Slack,
    Discord,
}

impl Destination {
    const ALL: [Self; 2] = [Self::Slack, Self::Discord];

    const fn name(self) -> &'static str {
        match self {
            Self::Slack => "slack",
            Self::Discord => "discord",
        }
    }

    const fn env_var(self) -> &'static str {
        match self {
            Self::Slack => "WARPGATE_TICKET_APPROVAL_SLACK_WEBHOOK",
            Self::Discord => "WARPGATE_TICKET_APPROVAL_DISCORD_WEBHOOK",
        }
    }

    fn accepts_url(self, url: &Url) -> bool {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && match self {
                Self::Slack => {
                    matches!(
                        url.host_str(),
                        Some("hooks.slack.com" | "hooks.slack-gov.com")
                    ) && url.path().starts_with("/services/")
                }
                Self::Discord => {
                    url.host_str() == Some("discord.com")
                        && url.path().starts_with("/api/webhooks/")
                }
            }
    }

    fn body(self, message: &str, approvals_url: Option<&Url>) -> Value {
        match self {
            Self::Slack => {
                let mut blocks = vec![json!({
                    "type": "section",
                    "text": {"type": "plain_text", "text": message},
                })];
                if let Some(url) = approvals_url {
                    blocks.push(json!({
                        "type": "actions",
                        "elements": [{
                            "type": "button",
                            "text": {"type": "plain_text", "text": "Review in Warpgate"},
                            "url": url.as_str(),
                        }],
                    }));
                }
                json!({"text": "Warpgate access request", "blocks": blocks, "unfurl_links": false})
            }
            Self::Discord => {
                let mut content = escape_discord(message);
                if let Some(url) = approvals_url {
                    content.push_str(&format!("\n<{url}>"));
                }
                json!({"content": content, "allowed_mentions": {"parse": []}})
            }
        }
    }
}

fn escape_discord(message: &str) -> String {
    let mut escaped = String::new();
    for c in message.chars() {
        if matches!(
            c,
            '\\' | '*' | '_' | '~' | '`' | '|' | '>' | '[' | ']' | '<' | '#'
        ) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// Use the same transaction that creates the pending request so the request and
/// its notifications become visible together, even after a process restart.
pub(crate) async fn enqueue(
    connection: &impl ConnectionTrait,
    request_id: Uuid,
) -> Result<(), DbErr> {
    Outbox::Entity::insert_many(Destination::ALL.map(|destination| Outbox::ActiveModel {
        request_id: Set(request_id),
        destination: Set(destination.name().into()),
        next_attempt_at: Set(OffsetDateTime::now_utc()),
        delivered_at: Set(None),
    }))
    .exec(connection)
    .await?;
    Ok(())
}

pub fn spawn(db: &DatabaseConnection) -> Result<()> {
    let mut destinations = Vec::new();
    for kind in Destination::ALL {
        let value = match std::env::var(kind.env_var()) {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => continue,
            Err(_) => anyhow::bail!("{} must be UTF-8", kind.env_var()),
        };
        // Avoid including the credential-bearing URL in errors.
        let url = Url::parse(&value).map_err(|_| anyhow::anyhow!("Invalid {}", kind.env_var()))?;
        ensure!(
            kind.accepts_url(&url),
            "Invalid endpoint in {}",
            kind.env_var()
        );
        destinations.push((kind, url));
    }
    if destinations.is_empty() {
        return Ok(());
    }

    let approvals_url = match std::env::var("WARPGATE_TICKET_APPROVALS_URL") {
        Ok(value) => {
            let url = Url::parse(&value).context("Invalid WARPGATE_TICKET_APPROVALS_URL")?;
            ensure!(
                url.scheme() == "https" && url.username().is_empty() && url.password().is_none(),
                "WARPGATE_TICKET_APPROVALS_URL must be an HTTPS URL"
            );
            Some(url)
        }
        Err(std::env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };

    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(8))
        .build()?;
    for (kind, url) in destinations {
        let db = db.clone();
        let client = client.clone();
        let approvals_url = approvals_url.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) =
                    deliver_due(&db, &client, kind, &url, approvals_url.as_ref()).await
                {
                    error!(%error, destination = kind.name(), "Ticket notification outbox unavailable");
                }
                tokio::time::sleep(Duration::from_secs(10)).await;
            }
        });
    }
    Ok(())
}

async fn deliver_due(
    db: &DatabaseConnection,
    client: &Client,
    kind: Destination,
    url: &Url,
    approvals_url: Option<&Url>,
) -> Result<(), DbErr> {
    let due = Outbox::Entity::find()
        .filter(Outbox::Column::Destination.eq(kind.name()))
        .filter(Outbox::Column::DeliveredAt.is_null())
        .filter(Outbox::Column::NextAttemptAt.lte(OffsetDateTime::now_utc()))
        .order_by_asc(Outbox::Column::NextAttemptAt)
        .limit(25)
        .all(db)
        .await?;

    for item in due {
        let request_id = item.request_id;
        // Claim one delivery at a time across replicas. The claim also schedules
        // its retry if the worker dies before recording a successful response.
        let claim = Outbox::Entity::update_many()
            .col_expr(
                Outbox::Column::NextAttemptAt,
                Expr::value(OffsetDateTime::now_utc() + time::Duration::minutes(2)),
            )
            .filter(Outbox::Column::RequestId.eq(request_id))
            .filter(Outbox::Column::Destination.eq(kind.name()))
            .filter(Outbox::Column::DeliveredAt.is_null())
            .filter(Outbox::Column::NextAttemptAt.eq(item.next_attempt_at))
            .exec(db)
            .await?;
        if claim.rows_affected != 1 {
            continue;
        }

        let Some(request) = TicketRequest::Entity::find_by_id(request_id)
            .one(db)
            .await?
        else {
            continue;
        };
        let user = User::Entity::find_by_id(request.user_id).one(db).await?;
        let target = Target::Entity::find_by_id(request.target_id)
            .one(db)
            .await?;
        let user = user
            .as_ref()
            .map_or("deleted user", |user| user.username.as_str());
        let target = target
            .as_ref()
            .map_or("deleted target", |target| target.name.as_str());
        // Reasons may contain sensitive content. Keep them in Warpgate.
        let user: String = user.chars().take(100).collect();
        let target: String = target.chars().take(100).collect();
        let scope = if request.permanent {
            "permanent"
        } else {
            "temporary"
        };
        let message = format!(
            "Warpgate {scope} access request\nUser: {user}\nTarget: {target}\nRequest: {request_id}"
        );
        let result = client
            .post(url.clone())
            .json(&kind.body(&message, approvals_url))
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => {
                warn!(%request_id, destination = kind.name(), status = %response.status(), "Ticket notification will be retried");
                continue;
            }
            Err(_) => {
                warn!(%request_id, destination = kind.name(), "Ticket notification delivery failed and will be retried");
                continue;
            }
        }
        Outbox::Entity::update_many()
            .col_expr(
                Outbox::Column::DeliveredAt,
                Expr::value(OffsetDateTime::now_utc()),
            )
            .filter(Outbox::Column::RequestId.eq(request_id))
            .filter(Outbox::Column::Destination.eq(kind.name()))
            .exec(db)
            .await?;
        info!(%request_id, destination = kind.name(), "Ticket request notification delivered");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_provider_webhook_origins_are_allowed() {
        for (kind, valid, invalid) in [
            (
                Destination::Slack,
                "https://hooks.slack.com/services/a/b/c",
                "https://hooks.slack.com.example.org/services/a/b/c",
            ),
            (
                Destination::Discord,
                "https://discord.com/api/webhooks/123/token",
                "https://discord.com@invalid.example/api/webhooks/123/token",
            ),
        ] {
            assert!(kind.accepts_url(&Url::parse(valid).unwrap()));
            assert!(!kind.accepts_url(&Url::parse(invalid).unwrap()));
        }
    }

    #[test]
    fn user_content_cannot_trigger_provider_mentions() {
        assert_eq!(
            Destination::Discord.body("@everyone", None)["allowed_mentions"]["parse"],
            json!([])
        );
        assert_eq!(
            Destination::Slack.body("<!channel>", None)["blocks"][0]["text"]["type"],
            "plain_text"
        );
    }
}
