use std::fs::{File, ReadDir, read_dir, read_to_string};
use std::io::{Write, Result};
use std::path::PathBuf;
use std::collections::{HashMap};
use tantivy::{TantivyDocument, IndexWriter};
use mini_search_engine::index;

#[derive(serde::Deserialize)]
struct Webpage {
	links: Vec<String>,
	title: String,
	headings: String,
	text: String,
	code: String,
}

struct Domain {
	name: String,
	pages: Vec<(String, Webpage)>,
	page_ranks: Vec<f64>,
}

fn main() -> tantivy::Result<()> {
	let args: Vec<String> = std::env::args().collect();
	if args.len() != 3 {
		eprintln!("Usage: {} CRAWLER_OUTPUT INDEX", args.get(0).unwrap_or(&"create_index".to_string()));
		std::process::exit(1);
	}
	let input_path = args[1].as_str();
	let index_path = args[2].as_str();

	let schema = index::get_schema();
	let index = index::create_index(&schema, index_path)?;
	let mut writer: IndexWriter = index.writer(512 * 1024 * 1024)?;
	let mut domain_list = File::create(index_path.to_string() + "/domains.txt")?;

	for entry in read_dir(input_path)? {
		let path = entry?.path();
		if !path.is_dir() {
			continue
		}
		let mut name = get_path_name(path.clone());

		println!("Indexing {}...", name);
		let domain = get_domain(name.clone(), read_dir(path)?)?;

		name.push('\n');
		domain_list.write_all(name.as_bytes())?;

		for (i, (url, webpage)) in domain.pages.iter().enumerate() {
			let mut document = TantivyDocument::new();
			document.add_text(schema.domain, domain.name.as_str());
			document.add_text(schema.url, url.as_str());
			document.add_u64(schema.page_rank, (domain.page_ranks[i] * std::u64::MAX as f64) as u64);
			document.add_text(schema.title, webpage.title.as_str());
			document.add_text(schema.headings, webpage.title.as_str());
			document.add_text(schema.headings, webpage.headings.as_str());
			document.add_text(schema.text, webpage.text.as_str());
			document.add_text(schema.code, webpage.code.as_str());
			writer.add_document(document)?;
		}
	}

	println!("Committing...");
	writer.commit()?;

	println!("Done");
	Ok(())
}

fn get_domain(name: String, dir: ReadDir) -> Result<Domain> {
	let mut pages = Vec::new();
	for entry in dir {
		let path = entry?.path();
		if !path.is_file() {
			continue
		}

		let mut url = get_path_name(path.clone()).replace("%2F", "/");
		if !url.ends_with(".json") {
			continue
		}
		url.truncate(url.len() - 5);

		let contents = read_to_string(&path)?;
		pages.push((url, serde_json::from_str(&contents)?));
	}
	let page_ranks = get_page_ranks(&pages);
	Ok(Domain {name, pages, page_ranks})
}

fn get_page_ranks(pages: &Vec<(String, Webpage)>) -> Vec<f64> {
	let mut page_indices = HashMap::new();
	for (i, (url, _)) in pages.iter().enumerate() {
		page_indices.insert(url, i);
	}

	let mut page_ranks = Vec::new();
	let mut outbound = Vec::new();
	let mut graph = vec![Vec::new(); pages.len()];

	let init = 1.0 / pages.len() as f64;
	for (i, (_, page)) in pages.iter().enumerate() {
		page_ranks.push(init);
		outbound.push(1.0 / (page.links.len() + 1) as f64);
		graph[i].push(i);
		for link in &page.links {
			if let Some(ind) = page_indices.get(link) {
				graph[*ind].push(i);
			}
		}
	}

	let damp = 0.85;
	let iterations = 16;
	let inv_pages_len = 1.0 / pages.len() as f64;
	for _ in 0..iterations {
		let mut new_page_ranks = Vec::new();
		for node in &graph {
			let sum: f64 = node.iter().map(|n| page_ranks[*n] * outbound[*n]).sum();
			new_page_ranks.push((1.0 - damp) * inv_pages_len + damp * sum);
		}
		debug_assert!(new_page_ranks.len() == page_ranks.len());
		page_ranks = new_page_ranks;
	}

	page_ranks
}

fn get_path_name(path: PathBuf) -> String {
	path.file_name().unwrap().to_str().unwrap().to_string()
}
