use std::{collections::HashMap, sync::Arc};

use error_stack::IntoReport;
use redis_interface as redis;

use crate::{
    errors::{self, DrainerError},
    services,
};

pub type StreamEntries = Vec<(String, HashMap<String, String>)>;
pub type StreamReadResult = HashMap<String, StreamEntries>;

pub async fn is_stream_available(stream_index: u8, store: Arc<services::Store>) -> bool {
    let stream_key_flag = get_stream_key_flag(store.clone(), stream_index);

    match store
        .redis_conn
        .set_key_if_not_exist(stream_key_flag.as_str(), true)
        .await
    {
        Ok(resp) => resp == redis::types::SetnxReply::KeySet,
        Err(_e) => {
            // Add metrics or logs
            false
        }
    }
}

pub async fn read_from_stream(
    stream_name: &str,
    max_read_count: u64,
    redis: &redis::RedisConnectionPool,
) -> errors::DrainerResult<StreamReadResult> {
    // "0-0" id gives first entry
    let stream_id = "0-0";
    let entries = redis
        .stream_read_entries(stream_name, stream_id, Some(max_read_count))
        .await
        .map_err(DrainerError::from)
        .into_report()?;
    Ok(entries)
}

pub async fn trim_from_stream(
    stream_name: &str,
    minimum_entry_id: &str,
    redis: &redis::RedisConnectionPool,
) -> errors::DrainerResult<usize> {
    let trim_kind = redis::StreamCapKind::MinID;
    let trim_type = redis::StreamCapTrim::Exact;
    let trim_id = minimum_entry_id;
    let trim_result = redis
        .stream_trim_entries(stream_name, (trim_kind, trim_type, trim_id))
        .await
        .map_err(DrainerError::from)
        .into_report()?;

    // Since xtrim deletes entries below given id excluding the given id.
    // Hence, deleting the minimum entry id
    redis
        .stream_delete_entries(stream_name, minimum_entry_id)
        .await
        .map_err(DrainerError::from)
        .into_report()?;

    // adding 1 because we are deleting the given id too
    Ok(trim_result + 1)
}

pub async fn make_stream_available(
    stream_name_flag: &str,
    redis: &redis::RedisConnectionPool,
) -> errors::DrainerResult<()> {
    redis
        .delete_key(stream_name_flag)
        .await
        .map_err(DrainerError::from)
        .into_report()
}

pub fn parse_stream_entries<'a>(
    read_result: &'a StreamReadResult,
    stream_name: &str,
) -> errors::DrainerResult<(&'a StreamEntries, String)> {
    read_result
        .get(stream_name)
        .and_then(|entries| {
            entries
                .last()
                .map(|last_entry| (entries, last_entry.0.clone()))
        })
        .ok_or_else(|| {
            errors::DrainerError::RedisError(error_stack::report!(
                redis::errors::RedisError::NotFound
            ))
        })
        .into_report()
}

pub fn increment_stream_index(index: u8, total_streams: u8) -> u8 {
    if index == total_streams - 1 {
        0
    } else {
        index + 1
    }
}

pub(crate) fn get_stream_key_flag(store: Arc<services::Store>, stream_index: u8) -> String {
    format!("{}_in_use", get_drainer_stream_name(store, stream_index))
}

pub(crate) fn get_drainer_stream_name(store: Arc<services::Store>, stream_index: u8) -> String {
    store.drainer_stream(format!("shard_{stream_index}").as_str())
}
