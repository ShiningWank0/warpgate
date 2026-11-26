use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum TicketRequestNotifications {
    Table,
    RequestId,
    Destination,
    NextAttemptAt,
    DeliveredAt,
}

#[derive(DeriveIden)]
enum TicketRequests {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        use TicketRequestNotifications as N;
        manager
            .create_table(
                Table::create()
                    .table(N::Table)
                    .col(ColumnDef::new(N::RequestId).uuid().not_null())
                    .col(ColumnDef::new(N::Destination).string_len(16).not_null())
                    .col(
                        ColumnDef::new(N::NextAttemptAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(ColumnDef::new(N::DeliveredAt).timestamp_with_time_zone())
                    .primary_key(Index::create().col(N::RequestId).col(N::Destination))
                    .foreign_key(
                        ForeignKey::create()
                            .from(N::Table, N::RequestId)
                            .to(TicketRequests::Table, TicketRequests::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_ticket_notification_due")
                    .table(N::Table)
                    .col(N::Destination)
                    .col(N::DeliveredAt)
                    .col(N::NextAttemptAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(TicketRequestNotifications::Table)
                    .to_owned(),
            )
            .await
    }
}
