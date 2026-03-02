//! Identity read API handlers.

use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::NaiveDateTime;
use entities::{
    identity::{account_aliases, accounts, person_accounts, persons},
    prelude::*,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Serialize, Deserialize)]
struct PersonCursor {
    id: i64,
}

#[utoipa::path(
    get,
    path = "/api/v1/identity/persons",
    summary = "List identity persons",
    description = "Returns identity person rows ordered by person id ascending with cursor pagination.",
    params(CursorPaginationQuery),
    tag = "identity",
    operation_id = "listIdentityPersons",
    responses(
        (status = 200, description = "Identity persons", body = IdentityPersonsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_persons(
    State(app_state): State<AppState>,
    Query(query): Query<CursorPaginationQuery>,
) -> Result<Json<IdentityPersonsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let mut builder = Persons::find().order_by_asc(persons::Column::Id);

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: PersonCursor = decode_cursor(cursor)?;
        builder = builder.filter(persons::Column::Id.gt(cursor.id));
    }

    let mut rows = builder
        .limit(limit + 1)
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let next_cursor = rows
        .last()
        .map(|row| encode_cursor(&PersonCursor { id: row.id }))
        .transpose()?;

    Ok(Json(IdentityPersonsListResponse {
        data: rows.into_iter().map(to_person_dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/identity/persons/{person_id}/accounts",
    summary = "List accounts linked to a person",
    description = "Returns all accounts linked to an identity person. If the person exists but has no linked accounts yet, returns an empty `accounts` array.",
    params(("person_id" = i64, Path, description = "Identity person id")),
    tag = "identity",
    operation_id = "listIdentityPersonAccounts",
    responses(
        (status = 200, description = "Accounts linked to person", body = IdentityPersonAccountsResponse),
        (status = 404, description = "Person not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_person_accounts(
    State(app_state): State<AppState>,
    Path(person_id): Path<i64>,
) -> Result<Json<IdentityPersonAccountsResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;

    let person_exists = Persons::find_by_id(person_id)
        .one(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?
        .is_some();
    if !person_exists {
        return Err(ApiErrorResponse::not_found("Identity person not found"));
    }

    let memberships = PersonAccounts::find()
        .filter(person_accounts::Column::PersonId.eq(person_id))
        .order_by_asc(person_accounts::Column::AccountId)
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    let account_ids: Vec<i64> = memberships.into_iter().map(|row| row.account_id).collect();
    if account_ids.is_empty() {
        return Ok(Json(IdentityPersonAccountsResponse {
            person_id,
            accounts: Vec::new(),
        }));
    }

    let account_rows = Accounts::find()
        .filter(accounts::Column::Id.is_in(account_ids.clone()))
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    let alias_rows = AccountAliases::find()
        .filter(account_aliases::Column::AccountId.is_in(account_ids))
        .filter(account_aliases::Column::IsCurrent.eq(true))
        .order_by_asc(account_aliases::Column::AliasNormalized)
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut aliases_by_account: HashMap<i64, Vec<String>> = HashMap::new();
    for alias in alias_rows {
        aliases_by_account
            .entry(alias.account_id)
            .or_default()
            .push(alias.alias);
    }

    let mut accounts = account_rows
        .into_iter()
        .map(|row| IdentityAccountDto {
            platform_id: row.platform_id,
            scope: row.scope,
            external_id: row.external_id,
            current_aliases: aliases_by_account.remove(&row.id).unwrap_or_default(),
        })
        .collect::<Vec<_>>();

    accounts.sort_by(|left, right| {
        left.platform_id
            .cmp(&right.platform_id)
            .then(left.scope.cmp(&right.scope))
            .then(left.external_id.cmp(&right.external_id))
    });

    Ok(Json(IdentityPersonAccountsResponse {
        person_id,
        accounts,
    }))
}

fn to_person_dto(row: persons::Model) -> IdentityPersonDto {
    IdentityPersonDto {
        id: row.id,
        name: row.name,
        created_at: to_rfc3339(row.created_at),
        updated_at: to_rfc3339(row.updated_at),
    }
}

fn resolve_limit(limit: Option<u64>) -> u64 {
    limit.unwrap_or(20).clamp(1, 100)
}

#[allow(clippy::result_large_err)]
fn require_db(app_state: &AppState) -> Result<Arc<DatabaseConnection>, ApiErrorResponse> {
    app_state
        .db
        .clone()
        .ok_or_else(|| ApiErrorResponse::service_unavailable("Database connection unavailable"))
}

fn to_rfc3339(value: NaiveDateTime) -> String {
    value.and_utc().to_rfc3339()
}

#[allow(clippy::result_large_err)]
fn encode_cursor<T: Serialize>(payload: &T) -> Result<String, ApiErrorResponse> {
    let bytes = serde_json::to_vec(payload).map_err(|err| {
        ApiErrorResponse::internal_server_error("Failed to encode pagination cursor")
            .with_detail(err.to_string())
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[allow(clippy::result_large_err)]
fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Result<T, ApiErrorResponse> {
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|err| {
        ApiErrorResponse::bad_request("Invalid cursor value").with_detail(err.to_string())
    })?;

    serde_json::from_slice::<T>(&bytes).map_err(|err| {
        ApiErrorResponse::bad_request("Invalid cursor payload").with_detail(err.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct CursorFixture {
        id: i64,
    }

    #[test]
    fn cursor_roundtrip_works() {
        let encoded = encode_cursor(&CursorFixture { id: 17 }).expect("encode cursor");
        let decoded: CursorFixture = decode_cursor(&encoded).expect("decode cursor");
        assert_eq!(decoded, CursorFixture { id: 17 });
    }

    #[test]
    fn limit_is_bounded() {
        assert_eq!(resolve_limit(None), 20);
        assert_eq!(resolve_limit(Some(0)), 1);
        assert_eq!(resolve_limit(Some(33)), 33);
        assert_eq!(resolve_limit(Some(500)), 100);
    }
}
