
import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import "./App.css";


type CodeBlock = {
  language: string;
  filename: string | null;
  heading: string | null;
  code: string;
};

type MarkdownDocument = {
  path: string;
  markdown: string;
  blocks: CodeBlock[];
};

function App() {
  const [document, setDocument] = useState<MarkdownDocument | null>(null);
  const [outputDir, setOutputDir] = useState("");
  const [savedPaths, setSavedPaths] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [inputMode, setInputMode] = useState<"file" | "paste">("file");
  const [markdownInput, setMarkdownInput] = useState("");

  async function selectMarkdown() {
    setError("");
    setSavedPaths([]);

    try {
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          { name: "Markdown", extensions: ["md", "markdown"] },
        ],
      });

      if (!selected || Array.isArray(selected)) return;

      setBusy(true);
      const result = await invoke<MarkdownDocument>("load_markdown", {
        path: selected,
      });

      setDocument(result);
      setInputMode("file");
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function parsePastedMarkdown() {
    if (!markdownInput.trim()) {
      setError("Markdown を入力してください。");
      return;
    }

    setError("");
    setSavedPaths([]);
    setBusy(true);

    try {
      const blocks = await invoke<CodeBlock[]>("preview_markdown", {
        markdown: markdownInput,
      });

      setDocument({
        path: "直接入力した Markdown",
        markdown: markdownInput,
        blocks,
      });
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function selectOutputDirectory() {
    setError("");

    try {
      const selected = await open({
        multiple: false,
        directory: true,
      });

      if (typeof selected === "string") {
        setOutputDir(selected);
      }
    } catch (err) {
      setError(String(err));
    }
  }

  async function exportProjectZip() {
    if (!document || document.blocks.length === 0) return;

    setError("");
    setSavedPaths([]);
    setBusy(true);

    try {
      const zipPath = await save({
        title: "プロジェクト ZIP を保存",
        defaultPath: "project.zip",
        filters: [
          { name: "ZIP Archive", extensions: ["zip"] },
        ],
      });

      if (!zipPath) return;

      const paths = await invoke<string[]>("save_project_zip", {
        markdown: document.markdown,
        zipPath,
      });

      setSavedPaths(paths);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="app">
      <header className="topbar">
        <div>
          <p className="eyebrow">MARKDOWN TOOLKIT</p>
          <h1>mdbreaker</h1>
          <p className="subtitle">
            Markdown からコードブロックを抽出して、個別のファイルに保存。
          </p>
        </div>
        <span className="version">DESKTOP</span>
      </header>


      <section className="panel">
        <div className="section-heading">
          <div>
            <h2>1. Markdown 入力</h2>
            <p>ファイルを読み込むか、Markdown を直接貼り付けます。</p>
          </div>
        </div>

        <div className="input-tabs">
          <button
            className={inputMode === "file" ? "tab active" : "tab"}
            onClick={() => setInputMode("file")}
          >
            ファイルから読み込む
          </button>
          <button
            className={inputMode === "paste" ? "tab active" : "tab"}
            onClick={() => setInputMode("paste")}
          >
            Markdown を直接入力
          </button>
        </div>

        {inputMode === "file" ? (
          document ? (
            <div className="file-info">
              <span className="file-icon">MD</span>
              <div className="file-details">
                <strong>{document.path.split(/[\\/]/).pop()}</strong>
                <span className="muted path-text">{document.path}</span>
              </div>
              <span className="count">{document.blocks.length} blocks</span>
              <button onClick={selectMarkdown} disabled={busy}>
                別のファイル
              </button>
            </div>
          ) : (
            <div className="empty-state">
              <span className="empty-icon">↓</span>
              <strong>Markdown ファイルを選択してください</strong>
              <span>.md / .markdown</span>
              <button onClick={selectMarkdown} disabled={busy}>
                ファイルを選択
              </button>
            </div>
          )
        ) : (
          <div className="markdown-editor">
            <label htmlFor="markdown-input">Markdown ソース</label>
            <textarea
              id="markdown-input"
              value={markdownInput}
              onChange={(event) => setMarkdownInput(event.currentTarget.value)}
              placeholder={"# Python サンプル\n\n```python main.py\nprint(\"Hello, world!\")\n```\n\n```rust src/main.rs\nfn main() {\n    println!(\"Hello, world!\");\n}\n```"}
              spellCheck={false}
            />
            <div className="editor-footer">
              <span>{markdownInput.length.toLocaleString()} 文字</span>
              <button
                onClick={parsePastedMarkdown}
                disabled={busy || !markdownInput.trim()}
              >
                {busy ? "解析中..." : "Markdown を解析"}
              </button>
            </div>
          </div>
        )}
      </section>


      <section className="panel">
        <div className="section-heading">
          <div>
            <h2>2. 抽出結果</h2>
            <p>コードブロックの言語・ファイル名・内容を確認できます。</p>
          </div>
          <span className="count">{document?.blocks.length ?? 0} 件</span>
        </div>

        {!document ? (
          <p className="muted hint">先に Markdown ファイルを選択してください。</p>
        ) : document.blocks.length === 0 ? (
          <p className="muted hint">コードブロックが見つかりませんでした。</p>
        ) : (
          <div className="block-list">
            {document.blocks.map((block, index) => (
              <article className="code-card" key={`${index}-${block.filename ?? ""}`}>
                <div className="code-header">
                  <div>
                    <span className="block-number">
                      BLOCK {String(index + 1).padStart(2, "0")}
                    </span>
                    <strong>{block.filename ?? `code_${index + 1}`}</strong>
                    {block.heading && (
                      <span className="muted heading-name">{block.heading}</span>
                    )}
                  </div>
                  <span className="language">{block.language}</span>
                </div>
                <pre>
                  <code>{block.code}</code>
                </pre>
              </article>
            ))}
          </div>
        )}
      </section>

      <section className="panel">
        <div className="section-heading">
          <div>
            <h2>3. ファイルを保存</h2>
            <p>抽出したコードを個別のファイルとして出力します。</p>
          </div>
        </div>

        <div className="destination">
          <div className="destination-path">
            <span className="muted">保存先</span>
            <strong>{outputDir || "保存先が選択されていません"}</strong>
          </div>
          <button className="secondary" onClick={selectOutputDirectory} disabled={busy}>
            フォルダを選択
          </button>

        </div>

        <button
          className="primary-action"
          onClick={exportProjectZip}
          disabled={busy || !document || document.blocks.length === 0}
        >
          {busy ? "ZIP を生成中..." : "プロジェクトを ZIP に保存"}
        </button>

        {error && <p className="message error">{error}</p>}

        {savedPaths.length > 0 && (
          <div className="message success">
            <strong>{savedPaths.length} 件のファイルを保存しました。</strong>
            <ul>
              {savedPaths.map((path) => <li key={path}>{path}</li>)}
            </ul>
          </div>
        )}
      </section>

      <footer>mdbreaker · Rust + Tauri</footer>
    </main>
  );
}

export default App;
