use std::fs;
use chrono::{DateTime, Utc};
use tantivy::schema::{Schema, Field, TextFieldIndexing, TextOptions, Term, IndexRecordOption, STRING, STORED, FAST};
use tantivy::tokenizer::{TextAnalyzer, Token, Tokenizer, TokenStream,  LowerCaser, Stemmer, Language};
use tantivy::query::{Query, TermQuery, AllQuery};

#[derive(Clone)]
pub struct SearchEngineSchema {
	handle: Schema,
	pub domain: Field,
	pub url: Field,
	pub page_rank: Field,
	pub title: Field,
	pub headings: Field,
	pub text: Field,
	pub code: Field,
}

pub struct IndexStatistics {
	pub creation_time: String,
	pub size: u64,
	pub page_count: u64,
	pub domain_page_counts: Vec<(String, u64)>,
}

pub fn get_schema() -> SearchEngineSchema {
	let mut schema_builder = Schema::builder();
	schema_builder.add_text_field("domain", STRING | STORED | FAST);
	schema_builder.add_text_field("url", STRING | STORED | FAST);
	schema_builder.add_u64_field("page_rank", STORED | FAST);
	schema_builder.add_text_field("title", get_text_options("text").set_stored());
	schema_builder.add_text_field("headings", get_text_options("text"));
	schema_builder.add_text_field("text", get_text_options("text").set_stored());
	schema_builder.add_text_field("code", get_text_options("code"));
	let schema = schema_builder.build();
	SearchEngineSchema {
		handle: schema.clone(),
		domain: schema.get_field("domain").unwrap(),
		url: schema.get_field("url").unwrap(),
		page_rank: schema.get_field("page_rank").unwrap(),
		title: schema.get_field("title").unwrap(),
		headings: schema.get_field("headings").unwrap(),
		text: schema.get_field("text").unwrap(),
		code: schema.get_field("code").unwrap(),
	}
}

fn get_text_options(tokenizer: &str) -> TextOptions {
	let indexing = TextFieldIndexing::default()
		.set_tokenizer(tokenizer)
		.set_index_option(IndexRecordOption::WithFreqsAndPositions);
	TextOptions::default().set_indexing_options(indexing)
}

pub fn create_index(schema: &SearchEngineSchema, index_dir: &str) -> tantivy::Result<tantivy::Index> {
	fs::create_dir_all(index_dir)?;
	let index = tantivy::Index::create_in_dir(index_dir, schema.handle.clone())?;
	register_tokenizers(&index);
	Ok(index)
}

pub fn open_index(index_dir: &str) -> tantivy::Result<tantivy::Index> {
	let index = tantivy::Index::open_in_dir(index_dir)?;
	register_tokenizers(&index);
	Ok(index)
}

fn register_tokenizers(index: &tantivy::Index) {
	let manager = index.tokenizers();
	let text_analyzer = TextAnalyzer::builder(TextTokenizer::default())
                .filter(LowerCaser)
                .filter(Stemmer::new(Language::English))
                .build();
	manager.register("text", text_analyzer);
	manager.register("code", CodeTokenizer::default());
}

pub fn get_statistics(schema: &SearchEngineSchema, index: &tantivy::Index, index_dir: &str) -> tantivy::Result<IndexStatistics> {
	let reader = index.reader_builder().try_into().unwrap();
	let searcher = reader.searcher();
	let domains_path = index_dir.to_string() + "/domains.txt";

	let creation_time = if let Ok(creation_time) = fs::metadata(&domains_path)?.created() {
		let date_time: DateTime<Utc> = creation_time.into();
		date_time.to_rfc2822()
	} else {
		"Unknown".to_string()
	};

	let size = get_size(index_dir)?;

	let all_query = AllQuery {};
	let page_count = all_query.count(&searcher)?.try_into().unwrap();

	let domains = fs::read_to_string(&domains_path)?;
	let domain_page_counts = domains.lines().map(|d| {
		let term = Term::from_field_text(schema.domain, d);
		let query = TermQuery::new(term, IndexRecordOption::Basic);
		let count = query.count(&searcher).unwrap_or(0);
		(d.to_string(), count.try_into().unwrap())
	}).collect::<Vec<_>>();

	Ok(IndexStatistics {
		creation_time,
		size,
		page_count,
		domain_page_counts,
	})
}

pub fn get_size(index_dir: &str) -> tantivy::Result<u64> {
	let mut total_size = 0u64;
	for entry in fs::read_dir(index_dir)? {
		let metadata = entry.unwrap().metadata()?;
		if metadata.is_file() {
			total_size += metadata.len();
		}
	}
	Ok(total_size)
}

#[derive(Clone, Default)]
pub struct TextTokenizer {
	token: Token,
}

pub struct TextTokenStream<'a> {
	text: &'a str,
	chars: std::str::CharIndices<'a>,
	token: &'a mut Token,
}

impl Tokenizer for TextTokenizer {
	type TokenStream<'a> = TextTokenStream<'a>;
	fn token_stream<'a>(&'a mut self, text: &'a str) -> TextTokenStream<'a> {
		self.token.reset();
		TextTokenStream {
			text,
			chars: text.char_indices(),
			token: &mut self.token,
		}
	}
}

impl TokenStream for TextTokenStream<'_> {
	fn advance(&mut self) -> bool {
		self.token.text.clear();
		self.token.position = self.token.position.wrapping_add(1);
		while let Some((offset_from, c)) = self.chars.next() {
			if is_word_char(c) {
				let offset_to = (&mut self.chars)
					.filter(|(_, c)| !is_word_char(*c))
					.map(|(offset, _)| offset)
					.next()
					.unwrap_or(self.text.len());
				self.token.offset_from = offset_from;
				self.token.offset_to = offset_to;
				self.token.text.push_str(&self.text[offset_from..offset_to]);
				return true;
			}
		}
		false
	}
	fn token(&self) -> &Token {
		self.token
	}
	fn token_mut(&mut self) -> &mut Token {
		self.token
	}
}

#[derive(Clone, Default)]
pub struct CodeTokenizer {
	token: Token,
}

pub struct CodeTokenStream<'a> {
	text: &'a str,
	chars: std::str::CharIndices<'a>,
	token: &'a mut Token,
}

impl Tokenizer for CodeTokenizer {
	type TokenStream<'a> = CodeTokenStream<'a>;
	fn token_stream<'a>(&'a mut self, text: &'a str) -> CodeTokenStream<'a> {
		self.token.reset();
		CodeTokenStream {
			text,
			chars: text.char_indices(),
			token: &mut self.token,
		}
	}
}

impl TokenStream for CodeTokenStream<'_> {
	fn advance(&mut self) -> bool {
	        self.token.text.clear();
	        self.token.position = self.token.position.wrapping_add(1);
	        while let Some((offset_from, c)) = self.chars.next() {
			if is_identifier_char(c) {
				let offset_to = (&mut self.chars)
					.filter(|(_, c)| !is_identifier_char(*c))
					.map(|(offset, _)| offset)
					.next()
					.unwrap_or(self.text.len());
				self.token.offset_from = offset_from;
				self.token.offset_to = offset_to;
				self.token.text.push_str(&self.text[offset_from..offset_to]);
				return true;
			}
			if !c.is_ascii_whitespace() {
				self.token.offset_from = offset_from;
				self.token.offset_to = offset_from + 1;
				self.token.text.push_str(&c.to_string());
				return true;
			}
	        }
	        false
	}
	fn token(&self) -> &Token {
		self.token
	}
	fn token_mut(&mut self) -> &mut Token {
		self.token
	}
}

// Match alphanumeric characters and a few other characters which shouldn't
// be ignored like punctuation
fn is_word_char(c: char) -> bool {
	c.is_alphanumeric() || "@#$%+-|".chars().any(|x| x == c)
}

// Very conservative language-agnostic tokenization of identifiers for
// CodeTokenizer
fn is_identifier_char(c: char) -> bool {
	c.is_ascii_alphanumeric() || !c.is_ascii()
}
