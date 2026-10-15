use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use std::collections::HashSet;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, serde::Serialize)]
pub struct CodeBlock {
    pub language: String,
    pub filename: Option<String>,
    pub heading: Option<String>,
    pub code: String,
}

/// Parse Markdown events and collect fenced code blocks.
/// The Markdown parser handles fenced blocks, so fence-like text inside code
/// is not mistaken for a new block.
pub fn extract_code_blocks(markdown: &str) -> Vec<CodeBlock> {
    let parser = Parser::new(markdown);
    let mut blocks = Vec::new();
    let mut current_heading: Option<String> = None;
    let mut heading_buffer: Option<String> = None;
    let mut active_block: Option<CodeBlock> = None;

    for event in parser {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                heading_buffer = Some(String::new());
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = heading_buffer.take() {
                    let heading = heading.trim().to_owned();
                    if !heading.is_empty() {
                        current_heading = Some(heading);
                    }
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let info = match kind {
                    CodeBlockKind::Fenced(info) => info.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                let (language, filename) = parse_info(&info);
                active_block = Some(CodeBlock {
                    language,
                    filename,
                    heading: current_heading.clone(),
                    code: String::new(),
                });
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(mut block) = active_block.take() {
                    // Markdown code blocks normally end with a newline. Remove
                    // just that final parser-added line ending.
                    if block.code.ends_with('\n') {
                        block.code.pop();
                        if block.code.ends_with('\r') {
                            block.code.pop();
                        }
                    }

                    if !block.code.is_empty() {
                        blocks.push(block);
                    }
                }
            }
            Event::Text(text) => {
                if let Some(block) = active_block.as_mut() {
                    block.code.push_str(&text);
                } else if let Some(heading) = heading_buffer.as_mut() {
                    heading.push_str(&text);
                }
            }
            Event::Code(text) => {
                if let Some(heading) = heading_buffer.as_mut() {
                    heading.push_str(&text);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(block) = active_block.as_mut() {
                    block.code.push('\n');
                } else if let Some(heading) = heading_buffer.as_mut() {
                    heading.push(' ');
                }
            }
            _ => {}
        }
    }

    blocks
}

/// Supported info-string forms:
/// - ```python filename=main.py
/// - ```rust:src/main.rs
fn parse_info(info: &str) -> (String, Option<String>) {
    let mut parts = info.split_whitespace();
    let first = parts.next().unwrap_or("text");

    let (language, inline_filename) = match first.split_once(':') {
        Some((language, filename)) if !filename.is_empty() => {
            (language.to_ascii_lowercase(), Some(filename.to_string()))
        }
        _ => (first.to_ascii_lowercase(), None),
    };

    let remaining: Vec<&str> = parts.collect();

    // filename=main.py / file=main.py を優先する
    let explicit_filename = remaining.iter().find_map(|part| {
        part.strip_prefix("filename=")
            .or_else(|| part.strip_prefix("file="))
            .filter(|name| !name.is_empty())
    });

    // python main.py のような形式にも対応
    let positional_filename = remaining
        .iter()
        .find(|part| !part.contains('=') && looks_like_filename(part))
        .copied();

    let filename = inline_filename
        .or_else(|| explicit_filename.map(str::to_string))
        .or_else(|| positional_filename.map(str::to_string));

    (language, filename)
}

fn extension_for(language: &str) -> &'static str {
    match language.to_ascii_lowercase().as_str() {
        "py" | "python" | "python3" => "py",
        "rs" | "rust" => "rs",
        "js" | "javascript" | "node" => "js",
        "jsx" => "jsx",
        "ts" | "typescript" => "ts",
        "tsx" => "tsx",
        "c" => "c",
        "h" => "h",
        "cc" | "cpp" | "c++" | "cxx" => "cpp",
        "hpp" | "hh" => "hpp",
        "java" => "java",
        "go" | "golang" => "go",
        "sh" | "bash" | "shell" | "zsh" => "sh",
        "html" | "xml" => "html",
        "css" => "css",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "sql" => "sql",
        "md" | "markdown" => "md",
        "txt" | "text" | "plaintext" => "txt",
        _ => "txt",
    }
}

fn looks_like_filename(value: &str) -> bool {
    let value = value.trim().trim_matches('`');
    let path = Path::new(value);

    path.file_name().is_some()
        && path.extension().is_some()
        && !value.chars().any(char::is_whitespace)
        && value.len() <= 180
}

/// Normalize a suggested filename and prevent absolute paths or `..` traversal
/// from writing outside the selected output directory.
fn sanitize_relative_path(input: &str, fallback_extension: &str) -> PathBuf {
    let normalized = input.trim().trim_matches('`').replace('\\', "/");
    let mut safe_parts = Vec::new();

    for part in normalized.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            continue;
        }

        let safe: String = part
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                    character
                } else {
                    '_'
                }
            })
            .collect();

        if !safe.is_empty() {
            safe_parts.push(safe);
        }
    }

    if safe_parts.is_empty() {
        return PathBuf::from(format!("code.{fallback_extension}"));
    }

    let mut path = safe_parts.iter().collect::<PathBuf>();
    if path.extension().is_none() {
        path.set_extension(fallback_extension);
    }

    path
}

/// Never overwrite an existing file: append `_2`, `_3`, ... as necessary.
fn unique_destination(output: &Path, relative: &Path) -> io::Result<PathBuf> {
    let candidate = output.join(relative);
    if !candidate.exists() {
        return Ok(candidate);
    }

    let parent = relative.parent().unwrap_or_else(|| Path::new(""));
    let stem = relative
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("code");
    let extension = relative
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("txt");

    for index in 2..=9999 {
        let filename = format!("{stem}_{index}.{extension}");
        let candidate = output.join(parent).join(filename);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not find an unused output filename",
    ))
}
/// Extract code blocks and save them as separate files.
/// Existing files are never overwritten.
pub fn save_code_blocks(markdown: &str, output: &Path) -> io::Result<Vec<String>> {
    std::fs::create_dir_all(output)?;

    let blocks = extract_code_blocks(markdown);
    let mut saved = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        let suggested_name = block
            .filename
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                let heading = block.heading.as_deref().unwrap_or("").trim();

                let base = if heading.is_empty() {
                    format!("code_{}", index + 1)
                } else {
                    heading.to_owned()
                };

                format!("{}.{}", base, extension_for(&block.language))
            });

        let relative = sanitize_relative_path(&suggested_name, extension_for(&block.language));

        let destination = unique_destination(output, &relative)?;

        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&destination, &block.code)?;

        saved.push(destination.display().to_string());
    }

    Ok(saved)
}

/// Extract Markdown code blocks into a ZIP archive.
/// Relative paths are preserved, and duplicate entries get numbered suffixes.
pub fn save_code_blocks_to_zip(markdown: &str, zip_path: &Path) -> io::Result<Vec<String>> {
    use zip::write::SimpleFileOptions;

    let blocks = extract_code_blocks(markdown);
    let file = std::fs::File::create(zip_path)?;
    let mut archive = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut used = HashSet::new();
    let mut saved = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        let suggested_name = block
            .filename
            .as_deref()
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("code_{}.{}", index + 1, extension_for(&block.language)));

        let relative = sanitize_relative_path(&suggested_name, extension_for(&block.language));

        let mut entry = relative.to_string_lossy().replace('\\', "/");

        if !used.insert(entry.clone()) {
            let parent = relative
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .to_string_lossy()
                .replace('\\', "/");

            let stem = relative
                .file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or("code");

            let extension = relative
                .extension()
                .and_then(OsStr::to_str)
                .unwrap_or("txt");

            let mut found = false;

            for suffix in 2..=9999 {
                let filename = format!("{stem}_{suffix}.{extension}");
                let candidate = if parent.is_empty() || parent == "." {
                    filename
                } else {
                    format!("{parent}/{filename}")
                };

                if used.insert(candidate.clone()) {
                    entry = candidate;
                    found = true;
                    break;
                }
            }

            if !found {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "could not find an unused ZIP entry name",
                ));
            }
        }

        archive
            .start_file(&entry, options)
            .map_err(|error| io::Error::other(error.to_string()))?;

        archive.write_all(block.code.as_bytes())?;

        saved.push(entry);
    }

    archive
        .finish()
        .map_err(|error| io::Error::other(error.to_string()))?;

    Ok(saved)
}

/// Best-effort syntax checks. This does not execute extracted programs.
fn check_file(language: &str, path: &Path) {
    let result = match language.to_ascii_lowercase().as_str() {
        "python" | "python3" | "py" => Command::new("python3")
            .args(["-m", "py_compile"])
            .arg(path)
            .status()
            .or_else(|_| {
                Command::new("python")
                    .args(["-m", "py_compile"])
                    .arg(path)
                    .status()
            }),
        "javascript" | "js" | "node" => Command::new("node").arg("--check").arg(path).status(),
        "rust" | "rs" => Command::new("rustc")
            .arg("--edition=2021")
            .arg("--emit=metadata")
            .arg(path)
            .arg("-o")
            .arg("/dev/null")
            .status(),
        "c" => Command::new("gcc").arg("-fsyntax-only").arg(path).status(),
        "cpp" | "c++" | "cc" | "cxx" => Command::new("g++").arg("-fsyntax-only").arg(path).status(),
        _ => {
            println!("  SKIP {} (no checker configured)", path.display());
            return;
        }
    };

    match result {
        Ok(status) if status.success() => println!("  OK   {}", path.display()),
        Ok(status) => println!("  FAIL {} (exit: {})", path.display(), status),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            println!("  SKIP {} (checker not installed)", path.display())
        }
        Err(error) => println!("  FAIL {} ({error})", path.display()),
    }
}
