use sea_orm::entity::prelude::*;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "ticket_request_notifications")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub request_id: Uuid,
    #[sea_orm(
        primary_key,
        auto_increment = false,
        column_type = "String(StringLen::N(16))"
    )]
    pub destination: String,
    pub next_attempt_at: OffsetDateTime,
    pub delivered_at: Option<OffsetDateTime>,
}

#[derive(Clone, Copy, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::TicketRequest::Entity",
        from = "Column::RequestId",
        to = "super::TicketRequest::Column::Id",
        on_delete = "Cascade"
    )]
    Request,
}

impl ActiveModelBehavior for ActiveModel {}
