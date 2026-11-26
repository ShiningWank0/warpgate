use sea_orm::{DatabaseConnection, EntityTrait};
use serde_json::json;
use warpgate_db_entities::User;

use super::*;
use crate::resources::tests::{db, target};
use crate::resources::{Actor, targets};

async fn requester(db: &DatabaseConnection, username: &str) -> UserId {
    let id = UserId(Uuid::new_v4());
    User::ActiveModel {
        id: Set(id),
        username: Set(username.into()),
        description: Set(String::new()),
        credential_policy: Set(json!({})),
        rate_limit_bytes_per_second: Set(None),
        ldap_server_id: Set(None),
        ldap_object_uuid: Set(None),
        allowed_ip_ranges: Set(serde_json::Value::Null),
        ..Default::default()
    }
    .insert(db)
    .await
    .unwrap();
    id
}

async fn request(
    db: &DatabaseConnection,
    user_id: UserId,
    target_id: TargetId,
    status: TicketRequestStatus,
    permanent: bool,
) -> Uuid {
    let id = Uuid::new_v4();
    TicketRequest::ActiveModel {
        id: Set(id),
        user_id: Set(user_id),
        target_id: Set(target_id),
        requested_duration_seconds: Set(Some(300)),
        permanent: Set(permanent),
        description: Set(String::new()),
        status: Set(status),
        resolved_by_user_id: Set(None),
        ticket_id: Set(None),
        created: Set(OffsetDateTime::now_utc()),
        resolved_at: Set(None),
        deny_reason: Set(None),
    }
    .insert(db)
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn security_invariant_only_the_owner_can_activate_an_approved_temporary_request() {
    let db = db().await;
    let owner = requester(&db, "owner").await;
    let other = requester(&db, "other").await;
    let target = targets::create(
        &db,
        Actor::Admin,
        &target("test", json!({"ssh": {"host": "test.invalid"}})),
    )
    .await
    .unwrap()
    .skip_audit();

    for (status, permanent, user) in [
        (TicketRequestStatus::Pending, false, owner),
        (TicketRequestStatus::Denied, false, owner),
        (TicketRequestStatus::Approved, true, owner),
        (TicketRequestStatus::Approved, false, other),
    ] {
        let id = request(&db, owner, target.id, status, permanent).await;
        assert!(matches!(
            activate_ticket_request(&db, id, user).await,
            Err(ActivateTicketRequestError::NotFound)
        ));
    }
    assert_eq!(Ticket::Entity::find().count(&db).await.unwrap(), 0);

    let id = request(&db, owner, target.id, TicketRequestStatus::Approved, false).await;
    let (a, b) = tokio::join!(
        activate_ticket_request(&db, id, owner),
        activate_ticket_request(&db, id, owner)
    );
    let (approved, duplicate) = if a.is_ok() { (a, b) } else { (b, a) };
    let approved = approved.unwrap();
    assert!(matches!(
        duplicate,
        Err(ActivateTicketRequestError::AlreadyActivated)
    ));
    let ticket = Ticket::Entity::find().one(&db).await.unwrap().unwrap();
    assert_eq!(Ticket::Entity::find().count(&db).await.unwrap(), 1);
    assert_eq!(approved.request.ticket_id, Some(ticket.id));
    assert_eq!(ticket.user_id, owner);
    assert_eq!(ticket.target_id, target.id);
}
