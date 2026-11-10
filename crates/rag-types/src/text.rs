//! Small text and path helpers shared by the server's CLI and the terminal client.

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// File name of a path-like source id. Understands both `/` and `\` so ids made on Windows and
/// on Unix display the same way.
pub fn short(source: &str) -> String {
    source.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(source).to_string()
}

/// Collapse whitespace and cut to `n` characters.
pub fn trunc(s: &str, n: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= n {
        flat
    } else {
        format!("{}…", flat.chars().take(n.saturating_sub(1)).collect::<String>())
    }
}

/// `████░░░░ 0.52`
pub fn score_bar(score: Option<f64>) -> String {
    match score {
        Some(sc) => {
            let n = (sc.clamp(0.0, 1.0) * 8.0).round() as usize;
            format!("{}{} {:.2}", "█".repeat(n), "░".repeat(8 - n), sc)
        }
        None => String::new(),
    }
}

/// Turn what a user typed or dragged into a terminal into a real path.
pub fn clean_path(input: &str) -> PathBuf {
    let mut s = input.trim().to_string();
    if (s.starts_with('\'') && s.ends_with('\'') || s.starts_with('"') && s.ends_with('"')) && s.len() >= 2 {
        s = s[1..s.len() - 1].to_string();
    }
    s = s.replace("\\ ", " ");
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(s)
}

/// Files under `path` (or `path` itself), skipping hidden files and folders.
pub fn collect_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let mut files: Vec<PathBuf> = WalkDir::new(path)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    files.sort();
    files
}

/// Source id for a file found under `root`: the folder name plus the path inside it, always with
/// forward slashes, so the same folder gets the same ids from any machine. A single file uses
/// just its file name.
pub fn source_id(root: &Path, file: &Path) -> String {
    if root.is_file() {
        return file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    }
    let base = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let rel: Vec<String> = file.strip_prefix(root).unwrap_or(file).components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
    if base.is_empty() {
        rel.join("/")
    } else {
        format!("{base}/{}", rel.join("/"))
    }
}

/// Tab-completion for filesystem paths: the longest common prefix of the matches.
pub fn complete_path(input: &str) -> Option<String> {
    let typed = clean_path(input);
    let sep_end = input.ends_with('/') || input.ends_with('\\');
    let (dir, prefix) = if sep_end {
        (typed.clone(), String::new())
    } else {
        (
            typed.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
            typed.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        )
    };
    let dir = if dir.as_os_str().is_empty() { PathBuf::from(".") } else { dir };
    let mut names: Vec<(String, bool)> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| Some((e.file_name().to_str()?.to_string(), e.path().is_dir())))
        .filter(|(n, _)| n.starts_with(&prefix) && (prefix.starts_with('.') || !n.starts_with('.')))
        .collect();
    names.sort();
    let first = names.first()?.clone();
    let mut common = first.0.clone();
    for (n, _) in &names {
        while !n.starts_with(&common) {
            common.pop();
        }
    }
    let completed = if names.len() == 1 && first.1 { format!("{common}/") } else { common };
    let base = input.rsplit_once(['/', '\\']).map(|(b, _)| format!("{b}/")).unwrap_or_default();
    Some(format!("{base}{completed}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trunc_and_short() {
        assert_eq!(trunc("a   b\nc", 10), "a b c");
        assert_eq!(trunc("abcdefghij", 5), "abcd…");
        assert_eq!(short("/a/b/c.pdf"), "c.pdf");
        assert_eq!(short("C:\\docs\\c.pdf"), "c.pdf", "Windows-style ids display the same way");
        assert_eq!(short("plain.txt"), "plain.txt");
    }

    #[test]
    fn clean_path_handles_drag_and_drop() {
        assert_eq!(clean_path("'/tmp/my file.txt'"), PathBuf::from("/tmp/my file.txt"));
        assert_eq!(clean_path("/tmp/my\\ file.txt"), PathBuf::from("/tmp/my file.txt"));
    }

    #[test]
    fn source_ids_are_portable() {
        let dir = std::env::temp_dir().join("rag_types_src_id");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let f = dir.join("sub").join("a.txt");
        std::fs::write(&f, "x").unwrap();
        assert_eq!(source_id(&dir, &f), "rag_types_src_id/sub/a.txt");
        assert_eq!(source_id(&f, &f), "a.txt", "a single file is just its name");
        std::fs::remove_dir_all(&dir).ok();
    }
}
