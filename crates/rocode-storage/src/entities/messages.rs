use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "messages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub session_id: String,
    pub role: String,
    pub created_at: i64,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub tokens_input: i64,
    pub tokens_output: i64,
    pub tokens_reasoning: i64,
    pub tokens_cache_read: i64,
    pub tokens_cache_write: i64,
    pub cost: f64,
    pub finish: Option<String>,
    pub metadata: Option<String>,
    pub data: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::sessions::Entity",
        from = "Column::SessionId",
        to = "super::sessions::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Session,
    #[sea_orm(has_many = "super::parts::Entity")]
    Parts,
}

impl Related<super::sessions::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Session.def()
    }
}

impl Related<super::parts::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Parts.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

