use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};

use crate::types::AppSettings;

const MAX_SEGMENT_CHARS: usize = 12_000;
const MAX_MERGE_INPUT_CHARS: usize = 18_000;
const MERGE_BATCH_SEPARATOR: &str = "\n\n---\n\n";

pub async fn summarize_transcript_text(
    transcript: &str,
    template: &str,
    settings: &AppSettings,
    api_key: &str,
) -> Result<(String, Value), String> {
    if transcript.trim().is_empty() {
        return Err("Transcript is empty".to_string());
    }
    if template.trim().is_empty() {
        return Err("Summary template is empty".to_string());
    }

    let segments = split_text(transcript, MAX_SEGMENT_CHARS);
    if segments.len() == 1 {
        let prompt = render_template(template, transcript);
        let response = call_chat_completion(settings, api_key, &prompt).await?;
        return Ok((
            response.content,
            json!({
                "strategy": "single_call",
                "segment_count": 1,
                "chunk_count": 1,
                "merge_rounds": 0,
                "max_segment_chars": MAX_SEGMENT_CHARS,
                "max_merge_input_chars": MAX_MERGE_INPUT_CHARS,
                "calls": [response.raw]
            }),
        ));
    }

    let segment_count = segments.len();
    let mut segment_summaries = Vec::new();
    let mut raw_calls = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let prompt = format!(
            "{}\n\n这是长文本的第 {}/{} 段。请只总结本段中和模板相关的事实，不要补充未出现的信息。",
            render_template(template, segment),
            index + 1,
            segment_count
        );
        let response = call_chat_completion(settings, api_key, &prompt).await?;
        raw_calls.push(response.raw);
        segment_summaries.push(format!("## 分段 {}\n{}", index + 1, response.content));
    }

    let (content, merge_rounds) = merge_summaries_recursively(
        template,
        segment_summaries,
        settings,
        api_key,
        &mut raw_calls,
    )
    .await?;
    Ok((
        content,
        json!({
            "strategy": "chunk_then_recursive_merge",
            "segment_count": segment_count,
            "chunk_count": segment_count,
            "merge_rounds": merge_rounds,
            "max_segment_chars": MAX_SEGMENT_CHARS,
            "max_merge_input_chars": MAX_MERGE_INPUT_CHARS,
            "calls": raw_calls
        }),
    ))
}

pub async fn test_provider(settings: &AppSettings, api_key: &str) -> Result<String, String> {
    let response =
        call_chat_completion(settings, api_key, "用一句中文回答：连接测试成功。").await?;
    Ok(response.content)
}

pub async fn polish_voice_input_text(
    text: &str,
    settings: &AppSettings,
    api_key: &str,
) -> Result<String, String> {
    if text.trim().is_empty() {
        return Err("语音输入转写结果为空".to_string());
    }
    let prompt = render_voice_input_prompt(&settings.voice_input_refinement_prompt, text);
    let response = call_chat_completion(settings, api_key, &prompt).await?;
    Ok(response.content)
}

fn render_voice_input_prompt(template: &str, transcript: &str) -> String {
    let template = template.trim();
    if template.is_empty() {
        return format!("请整理下面的语音输入文本，只输出最终文本：\n\n{transcript}");
    }
    if template.contains("{{ transcript }}") {
        return template.replace("{{ transcript }}", transcript.trim());
    }
    if template.contains("{{transcript}}") {
        return template.replace("{{transcript}}", transcript.trim());
    }
    if template.contains("{{ text }}") {
        return template.replace("{{ text }}", transcript.trim());
    }
    if template.contains("{{text}}") {
        return template.replace("{{text}}", transcript.trim());
    }
    format!("{template}\n\n转写文本：\n{}", transcript.trim())
}

struct LlmResponse {
    content: String,
    raw: Value,
}

async fn call_chat_completion(
    settings: &AppSettings,
    api_key: &str,
    prompt: &str,
) -> Result<LlmResponse, String> {
    let endpoint = chat_completions_endpoint(&settings.llm_provider, &settings.llm_base_url);
    let client = Client::builder()
        .timeout(Duration::from_secs(
            settings.llm_timeout_seconds.max(1) as u64
        ))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let body = json!({
        "model": settings.llm_model,
        "messages": [
            {
                "role": "system",
                "content": "你是一个严谨的会议整理助手。只依据用户提供的转写文本输出，无法确定的信息明确写未知。"
            },
            {
                "role": "user",
                "content": prompt
            }
        ],
        "temperature": settings.llm_temperature,
        "max_tokens": settings.llm_max_tokens
    });

    let response = client
        .post(endpoint)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("LLM request failed: {e}"))?;

    let status = response.status();
    let raw: Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse LLM response: {e}"))?;

    if !status.is_success() {
        return Err(format!("LLM returned HTTP {status}: {raw}"));
    }

    let content = raw
        .pointer("/choices/0/message/content")
        .and_then(|value| value.as_str())
        .or_else(|| {
            raw.pointer("/choices/0/text")
                .and_then(|value| value.as_str())
        })
        .ok_or_else(|| format!("LLM response did not include message content: {raw}"))?
        .trim()
        .to_string();

    if content.is_empty() {
        return Err("LLM returned empty content".to_string());
    }

    Ok(LlmResponse { content, raw })
}

fn render_template(template: &str, transcript: &str) -> String {
    if template.contains("{{ transcript }}") {
        template.replace("{{ transcript }}", transcript)
    } else if template.contains("{{transcript}}") {
        template.replace("{{transcript}}", transcript)
    } else {
        format!("{template}\n\n转写文本：\n{transcript}")
    }
}

async fn merge_summaries_recursively(
    template: &str,
    summaries: Vec<String>,
    settings: &AppSettings,
    api_key: &str,
    raw_calls: &mut Vec<Value>,
) -> Result<(String, usize), String> {
    if summaries.is_empty() {
        return Err("No segment summaries to merge".to_string());
    }

    let mut current = summaries;
    let mut merge_rounds = 0;

    loop {
        let batches = batch_texts(&current, MAX_MERGE_INPUT_CHARS);
        merge_rounds += 1;
        let batch_count = batches.len();
        let mut merged_round = Vec::new();

        for (index, batch) in batches.into_iter().enumerate() {
            let prompt = merge_prompt(template, &batch, merge_rounds, index + 1, batch_count);
            let response = call_chat_completion(settings, api_key, &prompt).await?;
            raw_calls.push(response.raw);
            merged_round.push(response.content);
        }

        if merged_round.len() == 1 {
            return Ok((merged_round.remove(0), merge_rounds));
        }

        current = merged_round
            .into_iter()
            .enumerate()
            .map(|(index, summary)| format!("## 合并摘要 {}\n{}", index + 1, summary))
            .collect();
    }
}

fn merge_prompt(
    template: &str,
    summaries: &[String],
    round: usize,
    batch_index: usize,
    batch_count: usize,
) -> String {
    let scope_instruction = if batch_count == 1 {
        "请把下面多个分段摘要合并为一份完整 Markdown 结果，去重、保留关键事实、明确待办、风险和跨文件主题，不要逐文件简单罗列。"
    } else {
        "请先把下面这一批分段摘要合并为中间 Markdown 摘要，去重并保留后续总合并必须使用的关键事实、明确待办和风险。"
    };

    format!(
        "你是一个严谨的会议纪要整理助手。{scope_instruction}\n\n合并轮次：第 {round} 轮，第 {batch_index}/{batch_count} 批。\n\n原始模板要求：\n{template}\n\n分段摘要：\n{}",
        summaries.join(MERGE_BATCH_SEPARATOR)
    )
}

fn batch_texts(texts: &[String], max_chars: usize) -> Vec<Vec<String>> {
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut current_chars = 0;

    for text in texts {
        let pieces = if char_count(text) > max_chars {
            split_text(text, max_chars)
        } else {
            vec![text.trim().to_string()]
        };

        for piece in pieces.into_iter().filter(|piece| !piece.trim().is_empty()) {
            let piece_chars = char_count(&piece);
            let separator_chars = if current.is_empty() {
                0
            } else {
                char_count(MERGE_BATCH_SEPARATOR)
            };
            if !current.is_empty() && current_chars + separator_chars + piece_chars > max_chars {
                batches.push(current);
                current = Vec::new();
                current_chars = 0;
            }

            current_chars += if current.is_empty() {
                piece_chars
            } else {
                separator_chars + piece_chars
            };
            current.push(piece);
        }
    }

    if !current.is_empty() {
        batches.push(current);
    }

    batches
}

fn split_text(text: &str, max_chars: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    if char_count(text) <= max_chars {
        return vec![text.to_string()];
    }

    let mut blocks = Vec::new();
    for block in split_by_markdown_boundaries(text) {
        if char_count(&block) <= max_chars {
            blocks.push(block);
        } else {
            blocks.extend(split_oversized_block(&block, max_chars));
        }
    }
    greedy_merge_chunks(blocks, "\n\n", max_chars)
}

fn split_by_markdown_boundaries(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let trimmed = line.trim_start();
        let starts_new_block = (trimmed.starts_with("## ") || trimmed.starts_with("### "))
            && !current.trim().is_empty();

        if starts_new_block {
            blocks.push(current.trim().to_string());
            current.clear();
        }

        current.push_str(line);
        current.push('\n');
    }

    if !current.trim().is_empty() {
        blocks.push(current.trim().to_string());
    }

    if blocks.is_empty() {
        vec![text.trim().to_string()]
    } else {
        blocks
    }
}

fn split_oversized_block(text: &str, max_chars: usize) -> Vec<String> {
    let paragraphs = split_paragraphs(text);
    if paragraphs
        .iter()
        .all(|paragraph| char_count(paragraph) <= max_chars)
    {
        return greedy_merge_chunks(paragraphs, "\n\n", max_chars);
    }

    let mut line_chunks = Vec::new();
    for paragraph in paragraphs {
        if char_count(&paragraph) <= max_chars {
            line_chunks.push(paragraph);
            continue;
        }

        let lines = paragraph
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if !lines.is_empty() && lines.iter().all(|line| char_count(line) <= max_chars) {
            line_chunks.extend(greedy_merge_chunks(lines, "\n", max_chars));
        } else {
            line_chunks.extend(split_oversized_lines(&paragraph, max_chars));
        }
    }

    greedy_merge_chunks(line_chunks, "\n\n", max_chars)
}

fn split_paragraphs(text: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join("\n").trim().to_string());
                current.clear();
            }
        } else {
            current.push(line);
        }
    }

    if !current.is_empty() {
        paragraphs.push(current.join("\n").trim().to_string());
    }

    if paragraphs.is_empty() {
        vec![text.trim().to_string()]
    } else {
        paragraphs
    }
}

fn split_oversized_lines(text: &str, max_chars: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();

    if lines.is_empty() {
        return split_sentences_or_chars(text, max_chars);
    }

    for line in lines {
        if char_count(line) <= max_chars {
            pieces.push(line.to_string());
        } else {
            pieces.extend(split_sentences_or_chars(line, max_chars));
        }
    }

    greedy_merge_chunks(pieces, "\n", max_chars)
}

fn split_sentences_or_chars(text: &str, max_chars: usize) -> Vec<String> {
    let mut sentences = text
        .split_inclusive(['。', '！', '？', '.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    if sentences.is_empty() {
        sentences = vec![text.trim().to_string()];
    }

    let mut pieces = Vec::new();
    for sentence in sentences {
        if char_count(&sentence) <= max_chars {
            pieces.push(sentence);
        } else {
            pieces.extend(split_by_chars(&sentence, max_chars));
        }
    }

    greedy_merge_chunks(pieces, "", max_chars)
}

fn split_by_chars(text: &str, max_chars: usize) -> Vec<String> {
    let chars = text.chars().collect::<Vec<_>>();
    chars
        .chunks(max_chars.max(1))
        .map(|chunk| chunk.iter().collect::<String>().trim().to_string())
        .filter(|chunk| !chunk.is_empty())
        .collect()
}

fn greedy_merge_chunks(chunks: Vec<String>, separator: &str, max_chars: usize) -> Vec<String> {
    let mut merged = Vec::new();
    let mut current = String::new();

    for chunk in chunks.into_iter().filter(|chunk| !chunk.trim().is_empty()) {
        let chunk = chunk.trim();
        let next_chars = if current.is_empty() {
            char_count(chunk)
        } else {
            char_count(&current) + char_count(separator) + char_count(chunk)
        };

        if !current.is_empty() && next_chars > max_chars {
            merged.push(current.trim().to_string());
            current.clear();
        }

        if current.is_empty() {
            current.push_str(chunk);
        } else {
            current.push_str(separator);
            current.push_str(chunk);
        }
    }

    if !current.trim().is_empty() {
        merged.push(current.trim().to_string());
    }

    merged
}

fn char_count(text: &str) -> usize {
    text.chars().count()
}

fn chat_completions_endpoint(_provider: &str, base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        return base.to_string();
    }
    if base.ends_with("/v1") {
        return format!("{base}/chat/completions");
    }
    format!("{base}/v1/chat/completions")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_not_split() {
        let chunks = split_text("短文本。", MAX_SEGMENT_CHARS);

        assert_eq!(chunks, vec!["短文本。"]);
    }

    #[test]
    fn markdown_boundaries_are_preserved_when_possible() {
        let text = format!(
            "# 本地空间：测试\n\n# 文本材料\n\n## 文本：A\n\n{}\n\n## 文本：B\n\n{}",
            "甲".repeat(200),
            "乙".repeat(200)
        );

        let chunks = split_text(&text, 260);

        assert!(chunks.len() >= 2);
        assert!(chunks.iter().any(|chunk| chunk.contains("## 文本：A")));
        assert!(chunks.iter().any(|chunk| chunk.contains("## 文本：B")));
        assert!(chunks.iter().all(|chunk| char_count(chunk) <= 260));
    }

    #[test]
    fn oversized_text_without_punctuation_is_force_split() {
        let text = "无".repeat(MAX_SEGMENT_CHARS * 2 + 333);

        let chunks = split_text(&text, MAX_SEGMENT_CHARS);

        assert!(chunks.len() >= 3);
        assert!(chunks
            .iter()
            .all(|chunk| char_count(chunk) <= MAX_SEGMENT_CHARS));
        assert_eq!(chunks.join("").chars().count(), text.chars().count());
    }

    #[test]
    fn merge_batches_respect_char_limit() {
        let summaries = (0..7)
            .map(|index| format!("## 分段 {}\n{}", index + 1, "内容".repeat(80)))
            .collect::<Vec<_>>();

        let batches = batch_texts(&summaries, 360);

        assert!(batches.len() > 1);
        assert!(batches
            .iter()
            .all(|batch| { char_count(&batch.join(MERGE_BATCH_SEPARATOR)) <= 360 }));
    }

    #[test]
    fn oversized_single_summary_is_split_for_merge_batching() {
        let summaries = vec!["长".repeat(1_000)];

        let batches = batch_texts(&summaries, 300);

        assert!(batches.len() > 1);
        assert!(batches
            .iter()
            .flat_map(|batch| batch.iter())
            .all(|part| char_count(part) <= 300));
    }
}
