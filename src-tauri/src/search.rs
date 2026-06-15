//! 로컬 본문 전문검색(P1b).
//!
//! tantivy 인덱스로 `.md` 본문/제목을 색인한다. 한글 부분일치를 위해
//! n-gram(2~3) 토크나이저를 쓰며, 문서 식별자는 볼트 상대경로다.
//! 인덱스는 앱 데이터 디렉터리에 두어(볼트 비오염) 볼트별 해시로 분리한다.
//! "FS가 진실, 인덱스는 파생 캐시" — 손상 시 재빌드(`>재색인`)로 복구한다.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tantivy::collector::TopDocs;
use tantivy::directory::MmapDirectory;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, Schema, TextFieldIndexing, TextOptions, Value, STORED, STRING};
use tantivy::snippet::SnippetGenerator;
use tantivy::tokenizer::{LowerCaser, NgramTokenizer, TextAnalyzer};
use tantivy::{doc, Index, IndexWriter, TantivyDocument, Term};

const TOKENIZER: &str = "ngram";
const WRITER_HEAP: usize = 50_000_000;

/// 프론트로 보내는 검색 히트(앱 IPC wire 타입).
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// 볼트 상대경로(POSIX 구분자).
    pub path: String,
    pub title: String,
    /// 매치 주변 본문 발췌.
    pub snippet: String,
    /// 스니펫 문자열 내 하이라이트 [start, end) char 인덱스.
    pub ranges: Vec<(usize, usize)>,
}

/// 스키마 필드 핸들(매 질의/색인마다 재조회 회피).
#[derive(Clone, Copy)]
pub struct Fields {
    pub path: Field,
    pub title: Field,
    pub body: Field,
}

/// path=식별자(비토큰, 정확삭제용), title/body=ngram 토큰.
pub fn build_schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    let path = b.add_text_field("path", STRING | STORED);
    let text = TextOptions::default()
        .set_indexing_options(
            TextFieldIndexing::default()
                .set_tokenizer(TOKENIZER)
                .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions),
        )
        .set_stored();
    let title = b.add_text_field("title", text.clone());
    let body = b.add_text_field("body", text);
    let schema = b.build();
    (schema, Fields { path, title, body })
}

/// 인덱스에 ngram 토크나이저를 등록한다(open/create 직후 필수).
pub fn register_tokenizer(index: &Index) {
    let ngram = NgramTokenizer::new(2, 3, false).expect("유효한 ngram 파라미터");
    let analyzer = TextAnalyzer::builder(ngram).filter(LowerCaser).build();
    index.tokenizers().register(TOKENIZER, analyzer);
}

/// 볼트 루트(canonical) → 앱 데이터 하위 인덱스 디렉터리.
pub fn index_dir(app_data: &Path, vault_root: &Path) -> PathBuf {
    let canon = std::fs::canonicalize(vault_root).unwrap_or_else(|_| vault_root.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(canon.to_string_lossy().as_bytes());
    let hex = format!("{:x}", hasher.finalize());
    app_data.join("index").join(&hex[..16])
}

/// 볼트 루트 기준 상대경로를 POSIX 구분자 문자열로.
fn rel_posix(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// `.md` 파일명(확장자 제외)을 제목으로.
fn title_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
}

/// 한 문서 upsert: 같은 path 문서를 지우고 새로 추가(교체 = idempotent).
pub fn upsert(
    writer: &IndexWriter,
    f: &Fields,
    rel_path: &str,
    title: &str,
    body: &str,
) -> tantivy::Result<()> {
    writer.delete_term(Term::from_field_text(f.path, rel_path));
    writer.add_document(doc!(
        f.path => rel_path,
        f.title => title,
        f.body => body,
    ))?;
    Ok(())
}

/// path로 문서 삭제(commit은 호출자 책임).
pub fn delete(writer: &IndexWriter, f: &Fields, rel_path: &str) {
    writer.delete_term(Term::from_field_text(f.path, rel_path));
}

/// 볼트의 모든 `.md`를 순회 색인(commit은 호출자). dot 디렉터리는 건너뛴다.
pub fn build_all(writer: &mut IndexWriter, f: &Fields, root: &Path) -> tantivy::Result<()> {
    index_dir_recursive(writer, f, root, root);
    Ok(())
}

fn index_dir_recursive(writer: &IndexWriter, f: &Fields, root: &Path, dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.filter_map(|r| r.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue; // .textree/.git 등 제외
        }
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            index_dir_recursive(writer, f, root, &path);
        } else if name.to_ascii_lowercase().ends_with(".md") {
            if let (Some(rel), Ok(body)) = (rel_posix(root, &path), std::fs::read_to_string(&path)) {
                let _ = upsert(writer, f, &rel, &title_of(&path), &body);
            }
        }
    }
}

/// body·title 다중필드 질의. 각 히트에 body 스니펫·하이라이트 범위를 붙인다.
/// 빈/공백 질의는 빈 결과.
pub fn search(index: &Index, f: &Fields, query: &str, limit: usize) -> tantivy::Result<Vec<SearchHit>> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let reader = index.reader()?;
    let searcher = reader.searcher();
    let parser = QueryParser::for_index(index, vec![f.title, f.body]);
    // 사용자 입력의 특수문자가 파싱 에러를 내지 않게 — 실패 시 빈 결과.
    let Ok(q) = parser.parse_query(query) else {
        return Ok(Vec::new());
    };
    let top = searcher.search(&q, &TopDocs::with_limit(limit))?;
    let mut snippet_gen = SnippetGenerator::create(&searcher, &q, f.body)?;
    snippet_gen.set_max_num_chars(160);

    let mut hits = Vec::with_capacity(top.len());
    for (_score, addr) in top {
        let doc: TantivyDocument = searcher.doc(addr)?;
        let path = first_text(&doc, f.path);
        let title = first_text(&doc, f.title);
        let snip = snippet_gen.snippet_from_doc(&doc);
        let fragment = snip.fragment().to_string();
        let ranges = byte_ranges_to_char(&fragment, snip.highlighted());
        hits.push(SearchHit { path, title, snippet: fragment, ranges });
    }
    Ok(hits)
}

fn first_text(doc: &TantivyDocument, field: Field) -> String {
    doc.get_first(field)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

/// tantivy 하이라이트(바이트 범위) → 스니펫 문자열의 char 인덱스 범위.
/// JS 문자열 슬라이스(UTF-16 BMP) 정합용. 보충문자(emoji)는 근사(무시 가능).
fn byte_ranges_to_char(s: &str, ranges: &[std::ops::Range<usize>]) -> Vec<(usize, usize)> {
    // 슬라이싱 없이 char_indices로 세어 코드포인트 중간 오프셋에도 패닉하지 않는다.
    let byte_to_char = |b: usize| s.char_indices().take_while(|(i, _)| *i < b).count();
    ranges.iter().map(|r| (byte_to_char(r.start), byte_to_char(r.end))).collect()
}

/// 현재 볼트의 인덱스 상태. 장수 IndexWriter를 보유해 빌드/증분 색인을 직렬화한다.
pub struct IndexState {
    pub index: Index,
    pub writer: IndexWriter,
    pub fields: Fields,
}

impl IndexState {
    /// 디렉터리에 인덱스를 열거나(없으면) 생성한다. 토크나이저를 등록한다.
    pub fn open_or_create(dir: &Path) -> tantivy::Result<Self> {
        std::fs::create_dir_all(dir).ok();
        let (schema, fields) = build_schema();
        let mmap = MmapDirectory::open(dir)?;
        let index = Index::open_or_create(mmap, schema)?;
        register_tokenizer(&index);
        let writer = index.writer(WRITER_HEAP)?;
        Ok(Self { index, writer, fields })
    }

    pub fn is_empty(&self) -> tantivy::Result<bool> {
        Ok(self.index.reader()?.searcher().num_docs() == 0)
    }

    /// 전체 재빌드: 기존 문서 전부 삭제 후 볼트를 다시 색인.
    pub fn rebuild(&mut self, vault_root: &Path) -> tantivy::Result<()> {
        self.writer.delete_all_documents()?;
        build_all(&mut self.writer, &self.fields, vault_root)?;
        self.writer.commit()?;
        Ok(())
    }

    /// 단일 파일 upsert/삭제를 writer에 스테이징한다(commit은 배치 호출자 책임).
    pub fn apply_change(&mut self, vault_root: &Path, abs_path: &Path, removed: bool) {
        if let Some(rel) = rel_posix(vault_root, abs_path) {
            if removed {
                delete(&self.writer, &self.fields, &rel);
            // 읽기 실패 시 기존 색인 유지(stale) — 워처 재발화/재색인으로 자가복구.
            } else if let Ok(body) = std::fs::read_to_string(abs_path) {
                let _ = upsert(&self.writer, &self.fields, &rel, &title_of(abs_path), &body);
            }
        }
    }

    /// 인메모리 본문으로 단일 노트를 색인하고 commit한다(앱내 편집 경로 — 워처가
    /// self-write를 억제하므로 쓰기 지점에서 결정적으로 인덱스를 갱신).
    pub fn index_note(&mut self, vault_root: &Path, abs_path: &Path, body: &str) -> tantivy::Result<()> {
        if let Some(rel) = rel_posix(vault_root, abs_path) {
            upsert(&self.writer, &self.fields, &rel, &title_of(abs_path), body)?;
            self.commit()?;
        }
        Ok(())
    }

    pub fn commit(&mut self) -> tantivy::Result<()> {
        self.writer.commit()?;
        Ok(())
    }

    pub fn search(&self, query: &str, limit: usize) -> tantivy::Result<Vec<SearchHit>> {
        search(&self.index, &self.fields, query, limit)
    }
}

/// Tauri State로 관리되는 현재 볼트 인덱스 핸들. 워처 스레드와 커맨드가 공유(Arc).
#[derive(Default)]
pub struct IndexHandle(pub Mutex<Option<IndexState>>);

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn num_docs(index: &Index) -> u64 {
        index.reader().unwrap().searcher().num_docs()
    }

    fn open_mem() -> (Index, Fields) {
        let (schema, f) = build_schema();
        let index = Index::create_in_ram(schema);
        register_tokenizer(&index);
        (index, f)
    }

    #[test]
    fn upsert_then_delete_roundtrips() {
        let (index, f) = open_mem();
        let mut w: IndexWriter = index.writer(WRITER_HEAP).unwrap();
        upsert(&w, &f, "일기/2026.md", "2026", "오늘 전문검색을 붙였다").unwrap();
        w.commit().unwrap();
        assert_eq!(num_docs(&index), 1);

        // 같은 path upsert = 교체(중복 아님).
        upsert(&w, &f, "일기/2026.md", "2026", "수정된 본문").unwrap();
        w.commit().unwrap();
        assert_eq!(num_docs(&index), 1, "동일 path는 교체되어 1건 유지");

        delete(&w, &f, "일기/2026.md");
        w.commit().unwrap();
        assert_eq!(num_docs(&index), 0);
    }

    #[test]
    fn build_all_indexes_md_bodies_with_relative_posix_path() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("일기")).unwrap();
        std::fs::write(root.join("노트.md"), "루트 본문").unwrap();
        std::fs::write(root.join("일기/2026.md"), "일기 본문").unwrap();
        std::fs::write(root.join("그림.png"), b"binary").unwrap(); // 비-md 제외
        std::fs::create_dir_all(root.join(".textree")).unwrap();
        std::fs::write(root.join(".textree/cache.md"), "캐시 본문").unwrap(); // dot 디렉터리 → 색인 제외

        let (index, f) = open_mem();
        let mut w = index.writer(WRITER_HEAP).unwrap();
        build_all(&mut w, &f, root).unwrap();
        w.commit().unwrap(); // build_all은 commit을 호출자에 위임 — 가시화 위해 commit.
        assert_eq!(num_docs(&index), 2, "dot 디렉터리 하위 .md는 제외되어 여전히 2건");

        // 상대경로가 POSIX 구분자로 저장되는지 — delete로 간접 확인.
        delete(&w, &f, "일기/2026.md");
        w.commit().unwrap();
        assert_eq!(num_docs(&index), 1);
    }

    #[test]
    fn query_matches_korean_substring_with_snippet() {
        let (index, f) = open_mem();
        let mut w = index.writer(WRITER_HEAP).unwrap();
        upsert(&w, &f, "a.md", "노트A", "오늘 전문검색 기능을 설계했다").unwrap();
        upsert(&w, &f, "b.md", "노트B", "고양이 사진을 붙였다").unwrap();
        w.commit().unwrap();

        let hits = search(&index, &f, "검색", 10).unwrap();
        assert_eq!(hits.len(), 1, "'검색'은 a.md만 매치");
        assert_eq!(hits[0].path, "a.md");
        assert!(hits[0].snippet.contains("검색"), "스니펫에 매치어 포함");
        assert!(!hits[0].ranges.is_empty(), "하이라이트 범위 존재");
    }

    #[test]
    fn empty_query_returns_no_hits() {
        let (index, f) = open_mem();
        let mut w = index.writer(WRITER_HEAP).unwrap();
        upsert(&w, &f, "a.md", "A", "본문").unwrap();
        w.commit().unwrap();
        assert!(search(&index, &f, "   ", 10).unwrap().is_empty());
    }

    #[test]
    fn open_or_create_then_rebuild_finds_doc_end_to_end() {
        let vault = TempDir::new().unwrap();
        std::fs::write(vault.path().join("메모.md"), "전문검색 통합 테스트").unwrap();
        let idx = TempDir::new().unwrap(); // 인덱스 디렉터리(앱 데이터 모사)

        let mut state = IndexState::open_or_create(idx.path()).unwrap();
        // 신규 인덱스는 비어있음 → 전체 빌드.
        state.rebuild(vault.path()).unwrap();

        let hits = state.search("검색", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "메모.md");
    }

    #[test]
    fn is_empty_reports_doc_presence() {
        let idx = TempDir::new().unwrap();
        let state = IndexState::open_or_create(idx.path()).unwrap();
        assert!(state.is_empty().unwrap(), "새 인덱스는 비어있음");
    }

    #[test]
    fn index_note_indexes_in_memory_body() {
        // 앱내 편집 경로 회귀 방어: 디스크 재읽기 없이 인메모리 본문이 색인된다.
        let vault = TempDir::new().unwrap();
        let idx = TempDir::new().unwrap();
        let abs = vault.path().join("일기/오늘.md");

        let mut state = IndexState::open_or_create(idx.path()).unwrap();
        state.index_note(vault.path(), &abs, "한글 본문 토큰").unwrap();

        let hits = state.search("토큰", 10).unwrap();
        assert_eq!(hits.len(), 1, "인메모리 본문이 검색에 잡힘");
        assert_eq!(hits[0].path, "일기/오늘.md", "상대경로(POSIX) 식별자 일치");
    }

    #[test]
    fn schema_has_three_fields() {
        let (schema, f) = build_schema();
        assert_eq!(schema.get_field("path").unwrap(), f.path);
        assert_eq!(schema.get_field("title").unwrap(), f.title);
        assert_eq!(schema.get_field("body").unwrap(), f.body);
    }

    #[test]
    fn index_dir_is_stable_and_distinct_per_root() {
        let app = Path::new("/app");
        let a1 = index_dir(app, Path::new("/no/such/vault-a"));
        let a2 = index_dir(app, Path::new("/no/such/vault-a"));
        let b = index_dir(app, Path::new("/no/such/vault-b"));
        assert_eq!(a1, a2, "같은 루트는 같은 디렉터리");
        assert_ne!(a1, b, "다른 루트는 다른 디렉터리");
        assert!(a1.starts_with(app.join("index")));
    }
}
