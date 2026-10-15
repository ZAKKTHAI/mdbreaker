use serde::Serialize;
use std::path::Path;

#[derive(Serialize)]
struct MarkdownDocument {
    path: String,
    markdown: String,
    blocks: Vec<mdbreaker_core::CodeBlock>,
}

#[tauri::command]
fn load_markdown(path: String) -> Result<MarkdownDocument, String> {
    let markdown = std::fs::read_to_string(&path)
        .map_err(|error| format!("Markdown の読み込みに失敗しました: {error}"))?;

    let blocks = mdbreaker_core::extract_code_blocks(&markdown);

    Ok(MarkdownDocument {
        path,
        markdown,
        blocks,
    })
}

#[tauri::command]
fn preview_markdown(markdown: String) -> Vec<mdbreaker_core::CodeBlock> {
    mdbreaker_core::extract_code_blocks(&markdown)
}

#[tauri::command]
fn save_project_zip(markdown: String, zip_path: String) -> Result<Vec<String>, String> {
    use std::path::{Path, PathBuf};

    let requested = PathBuf::from(&zip_path);

    let parent = requested
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));

    let stem = requested
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("project");

    let extension = requested
        .extension()
        .and_then(|name| name.to_str())
        .unwrap_or("zip");

    let mut destination = requested.clone();

    if destination.exists() {
        let mut found = false;

        for index in 1..=9999 {
            let candidate = parent.join(format!("{stem}-{index}.{extension}"));

            if !candidate.exists() {
                destination = candidate;
                found = true;
                break;
            }
        }

        if !found {
            return Err("空いている ZIP ファイル名が見つかりません".to_string());
        }
    }

    mdbreaker_core::save_code_blocks_to_zip(&markdown, &destination)
        .map_err(|error| format!("ZIP の保存に失敗しました: {error}"))
}

#[tauri::command]
fn save_code_blocks(markdown: String, output_dir: String) -> Result<Vec<String>, String> {
    mdbreaker_core::save_code_blocks(&markdown, Path::new(&output_dir))
        .map_err(|error| format!("ファイルの保存に失敗しました: {error}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            load_markdown,
            preview_markdown,
            save_code_blocks,
            save_project_zip
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
