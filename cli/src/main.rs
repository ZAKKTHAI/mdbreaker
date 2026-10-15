use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

#[derive(Debug)]
struct CodeBlock {
    language: String,
    filename: Option<String>,
    heading: Option<String>,
    code: String,
}

fn print_usage() {
    eprintln!(
        "mdbreaker — extract fenced code blocks from Markdown\n\
         \n\
         Usage:\n\
           mdbreaker <input.md> [-o|--output <directory>] [--check]\n\
         \n\
         Options:\n\
           -o, --output <directory>  Output directory (default: ./output)\n\
           --check                  Run available syntax checks after saving\n\
           -h, --help               Show this help\n\
         \n\
         Filename hints:\n\
           ```python filename=main.py\n\
           ```rust:src/main.rs"
    );
}

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args_os().skip(1);
    let mut input: Option<PathBuf> = None;
    let mut output = PathBuf::from("output");
    let mut check = false;

    while let Some(arg) = args.next() {
        if arg == OsStr::new("-h") || arg == OsStr::new("--help") {
            print_usage();
            return Ok(());
        } else if arg == OsStr::new("-o") || arg == OsStr::new("--output") {
            output = args
                .next()
                .map(PathBuf::from)
                .ok_or("missing directory after -o/--output")?;
        } else if arg == OsStr::new("--check") {
            check = true;
        } else if arg.to_string_lossy().starts_with('-') {
            return Err(format!("unknown option: {}", arg.to_string_lossy()).into());
        } else if input.is_none() {
            input = Some(PathBuf::from(arg));
        } else {
            return Err("only one input Markdown file can be specified".into());
        }
    }

    let input = match input {
        Some(path) => path,
        None => {
            print_usage();
            return Err("missing input Markdown file".into());
        }
    };

    let markdown = fs::read_to_string(&input)?;
    let blocks = extract_code_blocks(&markdown);

    if blocks.is_empty() {
        println!("No fenced code blocks found in {}", input.display());
        return Ok(());
    }

    fs::create_dir_all(&output)?;
    let mut written: Vec<(String, PathBuf)> = Vec::new();

    for (index, block) in blocks.iter().enumerate() {
        let extension = extension_for(&block.language);
        let proposed_name = block
            .filename
            .as_deref()
            .or_else(|| {
                block
                    .heading
                    .as_deref()
                    .filter(|heading| looks_like_filename(heading))
            })
            .map(str::to_owned)
            .unwrap_or_else(|| format!("code_{:03}.{}", index + 1, extension));

        let relative_path = sanitize_relative_path(&proposed_name, extension);
        let destination = unique_destination(&output, &relative_path)?;

        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&destination, &block.code)?;
        println!(
            "[{}] {}",
            if block.language.is_empty() {
                "text"
            } else {
                block.language.as_str()
            },
            destination.display()
        );

        written.push((block.language.clone(), destination));
    }

    println!(
        "Extracted {} code block(s) into {}",
        written.len(),
        output.display()
    );

    if check {
        println!("\nSyntax checks (only when a checker is installed):");
        for (language, path) in &written {
            check_file(language, path);
        }
    }

    Ok(())
}

/// Parse Markdown events and collect fenced code blocks.
/// The Markdown parser handles fenced blocks, so fence-like text inside code
/// is not mistaken for a new block.
fn extract_code_blocks(markdown: &str) -> Vec<CodeBlock> {
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
    let trimmed = info.trim();
    if trimmed.is_empty() {
        return ("text".to_string(), None);
    }

    let mut parts = trimmed.split_whitespace();
    let first = parts.next().unwrap_or("text");

    let (language, inline_filename) = match first.split_once(':') {
        Some((language, filename)) if !filename.is_empty() => {
            (language.to_ascii_lowercase(), Some(filename.to_string()))
        }
        _ => (first.to_ascii_lowercase(), None),
    };

    let filename = inline_filename.or_else(|| {
        parts.find_map(|part| {
            part.strip_prefix("filename=")
                .or_else(|| part.strip_prefix("file="))
                .map(str::to_string)
        })
    });

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
                if character.is_ascii_alphanumeric()
                    || matches!(character, '.' | '_' | '-')
                {
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
        "javascript" | "js" | "node" => Command::new("node")
            .arg("--check")
            .arg(path)
            .status(),
        "rust" | "rs" => Command::new("rustc")
            .arg("--edition=2021")
            .arg("--emit=metadata")
            .arg(path)
            .arg("-o")
            .arg("/dev/null")
            .status(),
        "c" => Command::new("gcc")
            .arg("-fsyntax-only")
            .arg(path)
            .status(),
        "cpp" | "c++" | "cc" | "cxx" => Command::new("g++")
            .arg("-fsyntax-only")
            .arg(path)
            .status(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_and_filename_hint() {
        assert_eq!(
            parse_info("python filename=main.py"),
            ("python".to_string(), Some("main.py".to_string()))
        );
        assert_eq!(
            parse_info("rust:src/main.rs"),
            ("rust".to_string(), Some("src/main.rs".to_string()))
        );
    }

    #[test]
    fn extracts_fenced_blocks() {
        let markdown = "# Example\n\n```python\nprint('hi')\n```\n";
        let blocks = extract_code_blocks(markdown);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "python");
        assert_eq!(blocks[0].code, "print('hi')");
        assert_eq!(blocks[0].heading.as_deref(), Some("Example"));
    }

    #[test]
    fn filename_detection_requires_extension() {
        assert!(looks_like_filename("main.py"));
        assert!(looks_like_filename("src/main.rs"));
        assert!(!looks_like_filename("Python example"));
    }

    #[test]
    fn path_traversal_is_removed() {
        let safe = sanitize_relative_path("../main.py", "py");
        assert_eq!(safe, PathBuf::from("main.py"));
    }
}
