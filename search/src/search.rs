use tantivy::{SegmentReader, DocId, Score};
use tantivy::schema::{Term, Value, Field, IndexRecordOption};
use tantivy::tokenizer::TextAnalyzer;
use tantivy::query::{Occur, Query, TermQuery, BoostQuery, PhraseQuery, BooleanQuery};
use tantivy::collector::TopDocs;
use crate::index::SearchEngineSchema;

struct UserQuery {
	domain: Option<String>,
	text_terms: String,
	text_phrases: Vec<String>,
	code_phrases: Vec<String>,
}

pub enum SearchResults {
	// Error caused by the user (currently always some sort of bad search query)
	// Empty string is special case that causes redirect to homepage
	// We could enumerate all possible error values here, but we're going
	// to create a string from it anyway, so this is simpler.
	Error(String),

	// Can be empty
	Entries(Vec<Entry>),
}

// One search result, a single webpage
pub struct Entry {
	pub title: String,
	pub url: String,
	pub excerpt: String,
}

// Limits on the size of the query to ensure that no searches can be made which
// would result in too much latency
const MAX_QUERY_STRING_LENGTH: usize = 16384;
const MAX_PHRASE_TOKENS: usize = 32;
const MAX_PHRASES: usize = 16;
const MAX_TERMS: usize = 128;

pub fn search(schema: &SearchEngineSchema, index: &tantivy::Index, query_string: &str) -> Option<SearchResults> {
	if query_string.len() > MAX_QUERY_STRING_LENGTH {
		return Some(SearchResults::Error(format!("Search query is too long! (max. {} characters)", MAX_QUERY_STRING_LENGTH)))
	}

	let user_query = parse_query(query_string);

	let reader = index.reader_builder().try_into().ok()?;
	let searcher = reader.searcher();

	let mut analyzer = index.tokenizers().get("text")?;
	let mut code_analyzer = index.tokenizers().get("code")?;

	let text_fields = &vec![
		(schema.headings, 8.0),
		(schema.text, 1.0),
	];

	// List of search terms which will be highlighted the excerpt for each
	// result
	let mut excerpt_highlight_terms = Vec::new();

	// Create tantivy queries from user query
	let text_phrase_queries = get_phrase_queries(&user_query.text_phrases, &mut analyzer, &text_fields, &mut excerpt_highlight_terms);
	let code_phrase_queries = get_phrase_queries(&user_query.code_phrases, &mut code_analyzer, &vec![(schema.code, 1.0)], &mut Vec::new());
	let domain_query = user_query.domain.map(|str| -> Box<dyn Query> {
		let term = Term::from_field_text(schema.domain, str.as_str());
		Box::new(TermQuery::new(term, IndexRecordOption::Basic))
	});
	let term_queries = get_term_queries(&user_query.text_terms, &mut analyzer, &text_fields, &mut excerpt_highlight_terms);

	if text_phrase_queries.is_empty() && code_phrase_queries.is_empty() && domain_query.is_none() && term_queries.is_empty() {
		// Empty string represents that no search was made because the query was empty
		return Some(SearchResults::Error("".to_string()));
	}

	// Construct full query and get top docs
	let queries: Vec<_> = text_phrase_queries.into_iter()
		.chain(code_phrase_queries.into_iter())
		.chain(domain_query.into_iter())
		.map(|q| (Occur::Must, q))
		.chain(term_queries.into_iter().map(|q| (Occur::Should, q)))
		.collect();
	let boolean_query = BooleanQuery::new(queries);
	let top_docs = searcher.search(&boolean_query, &TopDocs::with_limit(10).tweak_score(move |segment_reader: &SegmentReader| {
		let reader = segment_reader.fast_fields().u64("page_rank").unwrap().first_or_default_col(0);
		move |doc: DocId, original_score: Score| {
			let page_rank: u64 = reader.get_val(doc);
			let inv_u64_max = 1.0 / std::u64::MAX as f32;
			original_score * (page_rank as f32 * inv_u64_max).powf(0.15)
		}
	})).ok()?;

	// Get info for user from resulting documents
	let mut results = Vec::new();
	for (_, address) in top_docs {
		let retrieved_doc: tantivy::TantivyDocument = searcher.doc(address).ok()?;
		let text = retrieved_doc.get_first(schema.text).unwrap().as_str().unwrap();
	results.push(Entry {
			title: retrieved_doc.get_first(schema.title).unwrap().as_str().unwrap().to_string(),
			url: retrieved_doc.get_first(schema.url).unwrap().as_str().unwrap().to_string(),
			excerpt: get_excerpt(text, &excerpt_highlight_terms),
		});
	}

	Some(SearchResults::Entries(results))
}

// Very cheap way to get an excerpt from the text which matches the query.
// Tokenizer takes way too long, and tantivy doesn't seem to have a way to
// extract token data from the index once the tokens have already been indexed,
// so we just do simple string matching.
fn get_excerpt(text: &str, highlight_terms: &Vec<String>) -> String {
	// Just find the first search term in the full text and use that as
	// the excerpt.
	let index = highlight_terms.iter().filter_map(|pattern| text.find(pattern)).next().unwrap_or(0);

	let start = index.saturating_sub(32);
	let start = next_char_boundary(text, start);
	let end = text.len().min(start + 1024);
	let end = next_char_boundary(text, end);

	let mut excerpt = escape_html(&text[start..end]);
	for term in highlight_terms {
		let term = escape_html(term);

		// <b> tags might stack on top of each other, but the visible result is fine
		excerpt = excerpt.replace(&term, format!("<b>{}</b>", term).as_str());
	}
	excerpt
}

// Advance the index until you reach a char boundary. For splitting strings.
fn next_char_boundary(s: &str, mut i: usize) -> usize {
	while i < s.len() && !is_char_boundary(s.as_bytes()[i]) {
		i += 1;
	}
	i
}

// Returns true if the given byte is the first byte of a character encoded in UTF-8
fn is_char_boundary(b: u8) -> bool {
	!(0x80 <= b && b < 0xc0)
}

fn escape_html(input: &str) -> String {
	let mut res = String::new();
	for c in input.chars() {
		match c {
			'&' => res.push_str("&amp;"),
			'<' => res.push_str("&lt;"),
			'>' => res.push_str("&gt;"),
			'"' => res.push_str("&quot;"),
			'\'' => res.push_str("&apos;"),
			_ => res.push(c),
		}
	}
	res
}

fn get_phrase_queries(phrases: &Vec<String>, analyzer: &mut TextAnalyzer, fields: &Vec<(Field, f32)>, terms: &mut Vec<String>) -> Vec<Box<dyn Query>> {
	let mut queries: Vec<Box<dyn Query>> = Vec::new();
	for p in phrases.iter().take(MAX_PHRASES) {
		let mut token_stream = analyzer.token_stream(&p);
		let mut vs = vec![Vec::new(); fields.len()];
		while let Some(token) = token_stream.next() {
			terms.push(token.text.clone());
			for (v, (f, _)) in vs.iter_mut().zip(fields) {
				let term = Term::from_field_text(*f, token.text.as_str());
				v.push(term);
			}
			if vs[0].len() >= MAX_PHRASE_TOKENS {
				// Implicitly ignore any more tokens in phrase
				break
			}
		}
		debug_assert!(!vs.is_empty());
		if vs[0].len() > 1 {
			for (v, (_, b)) in vs.into_iter().zip(fields) {
				let q = Box::new(PhraseQuery::new(v));
				let q = Box::new(BoostQuery::new(q, *b));
				queries.push(q);
			}
			continue
		}
		for (v, (_, b)) in vs.into_iter().zip(fields) {
			queries.extend(v.into_iter().map(|term| -> Box<dyn Query> {
				let q = Box::new(TermQuery::new(term, IndexRecordOption::WithFreqs));
				Box::new(BoostQuery::new(q, *b))
			}));
		}
	}
	queries
}

fn get_term_queries(text_terms: &String, analyzer: &mut TextAnalyzer, fields: &Vec<(Field, f32)>, terms: &mut Vec<String>) -> Vec<Box<dyn Query>> {
	let mut res: Vec<Box<dyn Query>> = Vec::new();
	let mut token_stream = analyzer.token_stream(text_terms);
	while let Some(token) = token_stream.next() {
		terms.push(token.text.clone());
		for (f, b) in fields {
			let term = Term::from_field_text(*f, token.text.as_str());
			let q = Box::new(TermQuery::new(term, IndexRecordOption::WithFreqs));
			let q = Box::new(BoostQuery::new(q, *b));
			res.push(q);
		}
		if res.len() >= MAX_TERMS {
			// Implicitly ignore any more terms in search
			break
		}
	}
	res
}

// Parse the basic parts of the search query
fn parse_query(text: &str) -> UserQuery {
	let mut domain = None;
	let mut text_terms = String::new();
	let mut text_phrases = Vec::new();
	let mut code_phrases = Vec::new();

	let mut last_end = 0;
	let code = r#"`([^`]+)`|"([^`"]+)"|(\s|^)site:([a-z0-9-\.]+)"#;
	let re = regex::Regex::new(code).unwrap();
	for capture in re.captures_iter(text) {
		if let Some(x) = capture.get(1) {
			code_phrases.push(x.as_str().to_string())
		} else if let Some(x) = capture.get(2) {
			text_phrases.push(x.as_str().to_string())
		} else if let Some(x) = capture.get(4) {
			domain = Some(x.as_str().to_string())
		}
		let mat = capture.get(0).unwrap();
		if mat.start() > last_end {
			text_terms.push_str(&text[last_end..mat.start()]);
			text_terms.push(' ');
		}
		last_end = mat.end();
	}
	text_terms.push_str(&text[last_end..]);
	UserQuery {domain, text_terms, text_phrases, code_phrases}
}
