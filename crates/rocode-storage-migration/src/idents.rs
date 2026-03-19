use sea_orm_migration::prelude::*;

#[derive(DeriveIden)]
pub enum Sessions {
    Table,
    Id,
    ProjectId,
    ParentId,
    Slug,
    Directory,
    Title,
    Version,
    ShareUrl,
    SummaryAdditions,
    SummaryDeletions,
    SummaryFiles,
    SummaryDiffs,
    Revert,
    Permission,
    Metadata,
    UsageInputTokens,
    UsageOutputTokens,
    UsageReasoningTokens,
    UsageCacheWriteTokens,
    UsageCacheReadTokens,
    UsageTotalCost,
    Status,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum Messages {
    Table,
    Id,
    SessionId,
    Role,
    CreatedAt,
    ProviderId,
    ModelId,
    TokensInput,
    TokensOutput,
    TokensReasoning,
    TokensCacheRead,
    TokensCacheWrite,
    Cost,
    Finish,
    Metadata,
    Data,
}

#[derive(DeriveIden)]
pub enum Parts {
    Table,
    Id,
    MessageId,
    SessionId,
    CreatedAt,
    PartType,
    Text,
    ToolName,
    ToolCallId,
    ToolArguments,
    ToolResult,
    ToolError,
    ToolStatus,
    FileUrl,
    FileFilename,
    FileMime,
    Reasoning,
    SortOrder,
    Data,
}

#[derive(DeriveIden)]
pub enum Todos {
    Table,
    Id,
    SessionId,
    Content,
    Status,
    Priority,
    Position,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
pub enum Permissions {
    Table,
    ProjectId,
    CreatedAt,
    UpdatedAt,
    Data,
}

#[derive(DeriveIden)]
pub enum SessionShares {
    Table,
    Id,
    SessionId,
    ShareId,
    Secret,
    Url,
    CreatedAt,
}
