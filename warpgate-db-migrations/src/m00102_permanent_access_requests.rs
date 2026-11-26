use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum TicketRequests {
    Table,
    Permanent,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(TicketRequests::Table)
                    .add_column(
                        ColumnDef::new(TicketRequests::Permanent)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Downgrading would reinterpret approved permanent requests as
        // activatable tickets. Remove those non-ticket rows first.
        manager
            .exec_stmt(
                Query::delete()
                    .from_table(TicketRequests::Table)
                    .and_where(Expr::col(TicketRequests::Permanent).eq(true))
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(TicketRequests::Table)
                    .drop_column(TicketRequests::Permanent)
                    .to_owned(),
            )
            .await
    }
}
