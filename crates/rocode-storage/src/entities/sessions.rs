use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "sessions")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub parent_id: Option<i64>,
    pub directory: String,
    pub title: String,
    pub version: String,
    pub share_url: Option<String>,
    pub summary_additions: i64,
    pub summary_deletions: i64,
    pub summary_files: i64,
    pub summary_diffs: Option<String>,
    pub revert: Option<String>,
    pub permission: Option<String>,
    pub metadata: Option<String>,
    pub usage_input_tokens: i64,
    pub usage_output_tokens: i64,
    pub usage_reasoning_tokens: i64,
    pub usage_cache_write_tokens: i64,
    pub usage_cache_read_tokens: i64,
    pub usage_total_cost: f64,
    pub status: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::messages::Entity")]
    Messages,
    #[sea_orm(has_many = "super::parts::Entity")]
    Parts,
    #[sea_orm(has_many = "super::todos::Entity")]
    Todos,
    #[sea_orm(has_one = "super::session_shares::Entity")]
    SessionShare,
}

impl Related<super::messages::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Messages.def()
    }
}

impl Related<super::parts::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Parts.def()
    }
}

impl Related<super::todos::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Todos.def()
    }
}

impl Related<super::session_shares::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::SessionShare.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
