use std::time::Duration;

use sea_orm::QuerySelect;
use sea_orm::entity::prelude::*;
use sea_orm::sea_query::ExprTrait;
use time::OffsetDateTime;
use uuid::Uuid;
use warpgate_common::helpers::hash::hash_secret;
use warpgate_common::ticket_policy::{Redemption, decide_redemption};
use warpgate_common::{Secret, TargetId, UserId, WarpgateError};

use crate::begin_write;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "tickets")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    pub secret_hash: String,
    pub user_id: UserId,
    #[sea_orm(column_type = "Text")]
    pub description: String,
    pub target_id: TargetId,
    pub uses_left: Option<i16>,
    pub self_service: bool,
    pub expiry: Option<OffsetDateTime>,
    pub created: OffsetDateTime,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::TargetSession::Entity")]
    TargetSessions,
    #[sea_orm(
        belongs_to = "super::User::Entity",
        from = "Column::UserId",
        to = "super::User::Column::Id"
    )]
    User,
    #[sea_orm(
        belongs_to = "super::Target::Entity",
        from = "Column::TargetId",
        to = "super::Target::Column::Id"
    )]
    Target,
}

impl Related<super::User::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl Related<super::Target::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Target.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

pub async fn spend_use(db: &DatabaseConnection, ticket_id: Uuid) -> Result<(), WarpgateError> {
    redeem(db, ticket_id, None).await
}

/// Check the current row and commit its spend under the same lock. The clock
/// is sampled after the lock is obtained so contention cannot carry an old
/// authorization decision across expiry.
pub async fn redeem(
    db: &DatabaseConnection,
    ticket_id: Uuid,
    max_ttl: Option<Duration>,
) -> Result<(), WarpgateError> {
    let transaction = begin_write(db).await?;
    let Some(ticket) = Entity::find_by_id(ticket_id)
        .lock_exclusive()
        .one(&transaction)
        .await?
    else {
        return Err(WarpgateError::InvalidTicket(ticket_id));
    };

    match decide_redemption(
        ticket.created.unix_timestamp_nanos(),
        ticket.expiry.map(OffsetDateTime::unix_timestamp_nanos),
        ticket.uses_left,
        OffsetDateTime::now_utc().unix_timestamp_nanos(),
        max_ttl.map(|ttl| ttl.as_nanos()),
    ) {
        Redemption::Denied => return Err(WarpgateError::InvalidTicket(ticket_id)),
        Redemption::Unlimited => {}
        Redemption::Spend { remaining } => {
            let mut update: ActiveModel = ticket.into();
            update.uses_left = sea_orm::Set(Some(remaining));
            update.update(&transaction).await?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn refund_use(db: &DatabaseConnection, ticket_id: Uuid) -> Result<(), WarpgateError> {
    Entity::update_many()
        .col_expr(Column::UsesLeft, Expr::col(Column::UsesLeft).add(1))
        .filter(Column::Id.eq(ticket_id))
        .filter(Column::UsesLeft.is_not_null())
        .exec(db)
        .await?;
    Ok(())
}

pub async fn for_secret(
    db: &DatabaseConnection,
    secret: &Secret<String>,
) -> Result<Option<Uuid>, WarpgateError> {
    Ok(Entity::find()
        .filter(Column::SecretHash.eq(hash_secret(secret.expose_secret())))
        .one(db)
        .await?
        .map(|ticket| ticket.id))
}
