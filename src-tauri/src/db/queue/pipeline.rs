use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

use super::super::{connect, now_iso, SUMMARY_PIPELINE_STEPS, TRANSCRIPTION_PIPELINE_STEPS};
use super::local_jobs::parse_json_option;

pub(super) fn queue_table_and_label(
    queue_type: &str,
) -> Result<(&'static str, &'static str), String> {
    match queue_type {
        "transcription" => Ok(("transcription_jobs", "transcription")),
        "summary" => Ok(("summary_jobs", "summary")),
        _ => Err(format!("Unknown queue type: {queue_type}")),
    }
}

pub fn reset_local_queue_pipeline(queue_type: &str, job_id: &str) -> Result<(), String> {
    let conn = connect()?;
    let (table, label) = queue_table_and_label(queue_type)?;
    let now = now_iso();
    reset_pipeline(&conn, table, job_id, label, &now)
}

pub fn start_local_queue_pipeline_step(
    queue_type: &str,
    job_id: &str,
    step_name: &str,
    progress: i64,
) -> Result<(), String> {
    let conn = connect()?;
    let (table, label) = queue_table_and_label(queue_type)?;
    update_pipeline_step(
        &conn,
        table,
        job_id,
        label,
        step_name,
        "running",
        progress,
        None,
        &now_iso(),
        false,
    )
}

pub fn complete_local_queue_pipeline_step(
    queue_type: &str,
    job_id: &str,
    step_name: &str,
) -> Result<(), String> {
    let conn = connect()?;
    let (table, label) = queue_table_and_label(queue_type)?;
    update_pipeline_step(
        &conn,
        table,
        job_id,
        label,
        step_name,
        "completed",
        100,
        None,
        &now_iso(),
        true,
    )
}

pub(super) fn metadata_with_initialized_pipeline(
    metadata: Value,
    queue_type: &str,
    started_at: Option<&str>,
) -> Result<Value, String> {
    let mut metadata = ensure_object(metadata);
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            "pipeline".to_string(),
            initial_pipeline(queue_type, started_at)?,
        );
    }
    Ok(metadata)
}

fn initial_pipeline(queue_type: &str, started_at: Option<&str>) -> Result<Value, String> {
    let steps = pipeline_step_defs(queue_type)?
        .iter()
        .map(|(name, display_name)| {
            json!({
                "name": name,
                "display_name": display_name,
                "status": "pending",
                "progress": 0,
                "optional": false,
                "metadata": {}
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "steps": steps,
        "current_step_index": Value::Null,
        "total_progress": 0,
        "total_duration_ms": 0,
        "started_at": started_at,
        "completed_at": Value::Null
    }))
}

fn pipeline_step_defs(queue_type: &str) -> Result<&'static [(&'static str, &'static str)], String> {
    match queue_type {
        "transcription" => Ok(TRANSCRIPTION_PIPELINE_STEPS),
        "summary" => Ok(SUMMARY_PIPELINE_STEPS),
        _ => Err(format!("Unknown queue type: {queue_type}")),
    }
}

pub(super) fn start_pipeline(
    conn: &Connection,
    table: &str,
    job_id: &str,
    queue_type: &str,
    now: &str,
) -> Result<(), String> {
    update_pipeline(conn, table, job_id, now, |metadata| {
        let needs_init = metadata
            .get("pipeline")
            .and_then(|value| value.get("steps"))
            .and_then(|value| value.as_array())
            .map(|steps| steps.is_empty())
            .unwrap_or(true);
        if needs_init {
            metadata["pipeline"] = initial_pipeline(queue_type, Some(now))?;
        }
        if let Some(pipeline) = metadata
            .get_mut("pipeline")
            .and_then(|value| value.as_object_mut())
        {
            pipeline
                .entry("started_at".to_string())
                .or_insert_with(|| Value::String(now.to_string()));
            if pipeline
                .get("started_at")
                .and_then(|value| value.as_str())
                .is_none()
            {
                pipeline.insert("started_at".to_string(), Value::String(now.to_string()));
            }
            pipeline.insert("completed_at".to_string(), Value::Null);
        }
        Ok(())
    })
}

pub(super) fn reset_pipeline(
    conn: &Connection,
    table: &str,
    job_id: &str,
    queue_type: &str,
    now: &str,
) -> Result<(), String> {
    update_pipeline(conn, table, job_id, now, |metadata| {
        metadata["pipeline"] = initial_pipeline(queue_type, None)?;
        Ok(())
    })
}

pub(super) fn update_pipeline_step(
    conn: &Connection,
    table: &str,
    job_id: &str,
    queue_type: &str,
    step_name: &str,
    status: &str,
    progress: i64,
    error_message: Option<&str>,
    now: &str,
    complete_current: bool,
) -> Result<(), String> {
    update_pipeline(conn, table, job_id, now, |metadata| {
        let needs_init = metadata
            .get("pipeline")
            .and_then(|value| value.get("steps"))
            .and_then(|value| value.as_array())
            .map(|steps| steps.is_empty())
            .unwrap_or(true);
        if needs_init {
            metadata["pipeline"] = initial_pipeline(queue_type, Some(now))?;
        }
        let pipeline = metadata
            .get_mut("pipeline")
            .and_then(|value| value.as_object_mut())
            .ok_or_else(|| "Local queue pipeline metadata is not an object".to_string())?;
        if pipeline
            .get("started_at")
            .and_then(|value| value.as_str())
            .is_none()
        {
            pipeline.insert("started_at".to_string(), Value::String(now.to_string()));
        }
        pipeline.insert("completed_at".to_string(), Value::Null);

        let steps = pipeline
            .get_mut("steps")
            .and_then(|value| value.as_array_mut())
            .ok_or_else(|| "Local queue pipeline steps metadata is not an array".to_string())?;
        let step_index = steps
            .iter()
            .position(|step| step.get("name").and_then(|value| value.as_str()) == Some(step_name))
            .ok_or_else(|| format!("Unknown pipeline step {step_name} for {queue_type}"))?;

        for (index, step) in steps.iter_mut().enumerate() {
            let Some(step_object) = step.as_object_mut() else {
                continue;
            };
            if index < step_index {
                let status = step_object
                    .get("status")
                    .and_then(|value| value.as_str())
                    .unwrap_or("pending");
                if status != "completed" && status != "failed" && status != "skipped" {
                    step_object
                        .insert("status".to_string(), Value::String("completed".to_string()));
                    step_object.insert("progress".to_string(), Value::from(100));
                    step_object
                        .entry("started_at".to_string())
                        .or_insert_with(|| Value::String(now.to_string()));
                    step_object
                        .entry("completed_at".to_string())
                        .or_insert_with(|| Value::String(now.to_string()));
                    ensure_step_duration(step_object, now);
                }
            } else if index == step_index {
                step_object.insert("status".to_string(), Value::String(status.to_string()));
                step_object.insert("progress".to_string(), Value::from(progress.clamp(0, 100)));
                step_object
                    .entry("started_at".to_string())
                    .or_insert_with(|| Value::String(now.to_string()));
                if status == "completed" || status == "failed" || complete_current {
                    step_object.insert("completed_at".to_string(), Value::String(now.to_string()));
                    ensure_step_duration(step_object, now);
                } else {
                    step_object.remove("completed_at");
                    step_object.remove("duration_ms");
                }
                if let Some(message) = error_message {
                    step_object.insert(
                        "error".to_string(),
                        json!({
                            "message": message
                        }),
                    );
                    let metadata = step_object
                        .entry("metadata".to_string())
                        .or_insert_with(|| json!({}));
                    if let Some(metadata_object) = metadata.as_object_mut() {
                        metadata_object
                            .insert("error".to_string(), Value::String(message.to_string()));
                    }
                } else if status != "failed" {
                    step_object.remove("error");
                }
            }
        }
        pipeline.insert(
            "current_step_index".to_string(),
            Value::from(step_index as i64),
        );
        recalculate_pipeline_totals(pipeline);
        Ok(())
    })
}

pub(super) fn complete_pipeline(
    conn: &Connection,
    table: &str,
    job_id: &str,
    queue_type: &str,
    now: &str,
) -> Result<(), String> {
    update_pipeline(conn, table, job_id, now, |metadata| {
        let needs_init = metadata
            .get("pipeline")
            .and_then(|value| value.get("steps"))
            .and_then(|value| value.as_array())
            .map(|steps| steps.is_empty())
            .unwrap_or(true);
        if needs_init {
            metadata["pipeline"] = initial_pipeline(queue_type, Some(now))?;
        }
        let pipeline = metadata
            .get_mut("pipeline")
            .and_then(|value| value.as_object_mut())
            .ok_or_else(|| "Local queue pipeline metadata is not an object".to_string())?;
        if pipeline
            .get("started_at")
            .and_then(|value| value.as_str())
            .is_none()
        {
            pipeline.insert("started_at".to_string(), Value::String(now.to_string()));
        }
        let mut last_index = 0_i64;
        if let Some(steps) = pipeline
            .get_mut("steps")
            .and_then(|value| value.as_array_mut())
        {
            for (index, step) in steps.iter_mut().enumerate() {
                let Some(step_object) = step.as_object_mut() else {
                    continue;
                };
                step_object.insert("status".to_string(), Value::String("completed".to_string()));
                step_object.insert("progress".to_string(), Value::from(100));
                step_object
                    .entry("started_at".to_string())
                    .or_insert_with(|| Value::String(now.to_string()));
                step_object
                    .entry("completed_at".to_string())
                    .or_insert_with(|| Value::String(now.to_string()));
                ensure_step_duration(step_object, now);
                step_object.remove("error");
                last_index = index as i64;
            }
        }
        pipeline.insert("current_step_index".to_string(), Value::from(last_index));
        pipeline.insert("completed_at".to_string(), Value::String(now.to_string()));
        recalculate_pipeline_totals(pipeline);
        pipeline.insert("total_progress".to_string(), Value::from(100));
        Ok(())
    })
}

pub(super) fn fail_pipeline(
    conn: &Connection,
    table: &str,
    job_id: &str,
    queue_type: &str,
    error_message: &str,
    now: &str,
) -> Result<(), String> {
    update_pipeline(conn, table, job_id, now, |metadata| {
        let needs_init = metadata
            .get("pipeline")
            .and_then(|value| value.get("steps"))
            .and_then(|value| value.as_array())
            .map(|steps| steps.is_empty())
            .unwrap_or(true);
        if needs_init {
            metadata["pipeline"] = initial_pipeline(queue_type, Some(now))?;
        }
        let pipeline = metadata
            .get_mut("pipeline")
            .and_then(|value| value.as_object_mut())
            .ok_or_else(|| "Local queue pipeline metadata is not an object".to_string())?;
        if pipeline
            .get("started_at")
            .and_then(|value| value.as_str())
            .is_none()
        {
            pipeline.insert("started_at".to_string(), Value::String(now.to_string()));
        }
        let mut failed_index = 0_i64;
        if let Some(steps) = pipeline
            .get_mut("steps")
            .and_then(|value| value.as_array_mut())
        {
            let running_index = steps
                .iter()
                .position(|step| {
                    step.get("status").and_then(|value| value.as_str()) == Some("running")
                })
                .or_else(|| {
                    steps.iter().position(|step| {
                        step.get("status").and_then(|value| value.as_str()) == Some("pending")
                    })
                })
                .unwrap_or_else(|| steps.len().saturating_sub(1));
            failed_index = running_index as i64;
            for (index, step) in steps.iter_mut().enumerate() {
                let Some(step_object) = step.as_object_mut() else {
                    continue;
                };
                if index < running_index {
                    let status = step_object
                        .get("status")
                        .and_then(|value| value.as_str())
                        .unwrap_or("pending");
                    if status != "completed" && status != "skipped" {
                        step_object
                            .insert("status".to_string(), Value::String("completed".to_string()));
                        step_object.insert("progress".to_string(), Value::from(100));
                        step_object
                            .entry("started_at".to_string())
                            .or_insert_with(|| Value::String(now.to_string()));
                        step_object
                            .entry("completed_at".to_string())
                            .or_insert_with(|| Value::String(now.to_string()));
                        ensure_step_duration(step_object, now);
                    }
                } else if index == running_index {
                    step_object.insert("status".to_string(), Value::String("failed".to_string()));
                    step_object.insert("progress".to_string(), Value::from(100));
                    step_object
                        .entry("started_at".to_string())
                        .or_insert_with(|| Value::String(now.to_string()));
                    step_object.insert("completed_at".to_string(), Value::String(now.to_string()));
                    ensure_step_duration(step_object, now);
                    step_object.insert(
                        "error".to_string(),
                        json!({
                            "message": error_message
                        }),
                    );
                    let metadata = step_object
                        .entry("metadata".to_string())
                        .or_insert_with(|| json!({}));
                    if let Some(metadata_object) = metadata.as_object_mut() {
                        metadata_object.insert(
                            "error".to_string(),
                            Value::String(error_message.to_string()),
                        );
                    }
                }
            }
        }
        pipeline.insert("current_step_index".to_string(), Value::from(failed_index));
        pipeline.insert("completed_at".to_string(), Value::String(now.to_string()));
        recalculate_pipeline_totals(pipeline);
        Ok(())
    })
}

fn update_pipeline<F>(
    conn: &Connection,
    table: &str,
    job_id: &str,
    updated_at: &str,
    mutate: F,
) -> Result<(), String>
where
    F: FnOnce(&mut Value) -> Result<(), String>,
{
    let metadata_text: Option<String> = conn
        .query_row(
            &format!("SELECT metadata FROM {table} WHERE id = ?1"),
            params![job_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("Failed to read local queue job metadata: {e}"))?
        .ok_or_else(|| format!("Local queue job not found: {job_id}"))?;
    let mut metadata = ensure_object(parse_json_option(metadata_text).unwrap_or_else(|| json!({})));
    mutate(&mut metadata)?;
    conn.execute(
        &format!("UPDATE {table} SET metadata = ?1, updated_at = ?2 WHERE id = ?3"),
        params![metadata.to_string(), updated_at, job_id],
    )
    .map_err(|e| format!("Failed to update local queue job pipeline: {e}"))?;
    Ok(())
}

fn ensure_object(value: Value) -> Value {
    if value.is_object() {
        value
    } else {
        json!({})
    }
}

fn ensure_step_duration(step_object: &mut serde_json::Map<String, Value>, now: &str) {
    if step_object
        .get("duration_ms")
        .and_then(|value| value.as_i64())
        .is_some()
    {
        return;
    }
    let started_at = step_object
        .get("started_at")
        .and_then(|value| value.as_str())
        .unwrap_or(now);
    let completed_at = step_object
        .get("completed_at")
        .and_then(|value| value.as_str())
        .unwrap_or(now);
    let duration = chrono::DateTime::parse_from_rfc3339(completed_at)
        .ok()
        .zip(chrono::DateTime::parse_from_rfc3339(started_at).ok())
        .map(|(completed, started)| (completed - started).num_milliseconds().max(0))
        .unwrap_or(0);
    step_object.insert("duration_ms".to_string(), Value::from(duration));
}

fn recalculate_pipeline_totals(pipeline: &mut serde_json::Map<String, Value>) {
    let Some(steps) = pipeline.get("steps").and_then(|value| value.as_array()) else {
        pipeline.insert("total_progress".to_string(), Value::from(0));
        pipeline.insert("total_duration_ms".to_string(), Value::from(0));
        return;
    };
    if steps.is_empty() {
        pipeline.insert("total_progress".to_string(), Value::from(0));
        pipeline.insert("total_duration_ms".to_string(), Value::from(0));
        return;
    }

    let mut completed_weight = 0.0;
    let mut total_duration = 0_i64;
    for step in steps {
        let status = step
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("pending");
        if status == "completed" || status == "skipped" {
            completed_weight += 1.0;
        } else if status == "running" || status == "failed" {
            let progress = step
                .get("progress")
                .and_then(|value| value.as_i64())
                .unwrap_or(0)
                .clamp(0, 100);
            completed_weight += progress as f64 / 100.0;
        }
        total_duration += step
            .get("duration_ms")
            .and_then(|value| value.as_i64())
            .unwrap_or(0)
            .max(0);
    }
    let total_progress = ((completed_weight / steps.len() as f64) * 100.0)
        .floor()
        .clamp(0.0, 100.0) as i64;
    pipeline.insert("total_progress".to_string(), Value::from(total_progress));
    pipeline.insert("total_duration_ms".to_string(), Value::from(total_duration));
}

pub(super) fn transcription_pipeline_progress(progress: i64) -> Option<(&'static str, i64)> {
    match progress {
        0..=34 => Some(("prepare_audio_environment", scale_progress(progress, 5, 34))),
        35..=89 => Some(("run_asr", scale_progress(progress, 35, 89))),
        90..=94 => Some(("save_result", scale_progress(progress, 90, 94))),
        95..=99 => Some(("write_files", scale_progress(progress, 95, 99))),
        _ => None,
    }
}

pub(super) fn summary_pipeline_progress(progress: i64) -> Option<(&'static str, i64)> {
    match progress {
        0..=34 => Some(("load_material", scale_progress(progress, 10, 34))),
        55..=89 => Some(("call_llm", scale_progress(progress, 55, 89))),
        90..=94 => Some(("save_result", scale_progress(progress, 90, 94))),
        95..=99 => Some(("write_files", scale_progress(progress, 95, 99))),
        _ => None,
    }
}

fn scale_progress(progress: i64, start: i64, end: i64) -> i64 {
    if end <= start {
        return 100;
    }
    (((progress - start).max(0) as f64 / (end - start) as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as i64
}
