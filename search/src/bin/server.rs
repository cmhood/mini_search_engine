use std::time::Instant;
use actix_web::{web, App, HttpResponse, HttpRequest, HttpServer, Responder};
use askama_actix::Template;
use mini_search_engine::search;
use mini_search_engine::index;

#[derive(Template)]
#[template(path = "search.html")]
struct SearchTemplate<'a> {
	query: &'a str,
	latency: &'a str,
	results: search::SearchResults,
}

#[derive(Template)]
#[template(path = "stats.html")]
struct StatsTemplate {
	creation_time: String,
	index_size: String,
	index_page_count: u64,
	domain_page_counts: Vec<(String, u64)>,
}

#[derive(serde_derive::Deserialize)]
struct SearchQuery {
	q: Option<String>,
}

#[derive(Clone)]
struct AppData {
	schema: index::SearchEngineSchema,
	index: tantivy::Index,
	stats: String,
}

impl AppData {
	fn initialize(index_path: &str) -> tantivy::Result<AppData> {
		let schema = index::get_schema();
		let index = index::open_index(index_path)?;
		let stats = get_stats_template(&schema, &index, index_path)?.render().unwrap();
		Ok(AppData {schema, index, stats})
	}
}

fn get_stats_template(schema: &index::SearchEngineSchema, index: &tantivy::Index, index_path: &str) -> tantivy::Result<StatsTemplate> {
	let stats = index::get_statistics(schema, index, index_path)?;
	Ok(StatsTemplate {
		creation_time: stats.creation_time,
		index_size: format!("{:.1} MiB", stats.size as f32 * 2f32.powf(-20f32)),
		index_page_count: stats.page_count,
		domain_page_counts: stats.domain_page_counts,
	})
}

#[actix_web::get("/search")]
async fn serve_search(query: web::Query<SearchQuery>, data: web::Data<AppData>) -> impl Responder {
	let q = query.q.as_ref().map_or("", |s| s.as_str());

	let time = Instant::now();
	let results = search::search(&data.schema, &data.index, q);
	let latency = format!("{} seconds", time.elapsed().subsec_micros() as f32 * 0.000001f32);

	let tmpl = SearchTemplate {
		query: &q,
		latency: &latency,
		results: match results {
			None => search::SearchResults::Error("Internal server error".to_string()),
			Some(search::SearchResults::Error(e)) if e.is_empty() => {
				// Redirect to homepage to implicitly get the user to search with a new (valid) query
				return HttpResponse::TemporaryRedirect().insert_header(("LOCATION", "/")).finish()
			},
			Some(x) => x,
		},
	};
	HttpResponse::Ok().body(tmpl.render().unwrap())
}

#[actix_web::get("/stats")]
async fn serve_stats(data: web::Data<AppData>) -> impl Responder {
	HttpResponse::Ok().body(data.stats.clone())
}

async fn serve_default(req: HttpRequest) -> impl Responder {
	let body: &[u8] = match req.uri().path() {
		"/" => include_bytes!("../html/static/index.html"),
		"/syntax" => include_bytes!("../html/static/syntax.html"),
		"/style.css" => include_bytes!("../html/static/style.css"),
		"/bookshelf.jpeg" => include_bytes!("../html/static/bookshelf.jpeg"),
		_ => return HttpResponse::NotFound().body(&include_bytes!("../html/static/404.html")[..]),
	};
	HttpResponse::Ok().body(body)
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
	let args: Vec<String> = std::env::args().collect();
	if args.len() != 3 {
		eprintln!("Usage: {} INDEX ADDRESS", args.get(0).unwrap_or(&"server".to_string()));
		std::process::exit(1);
	}
	let index_path = args[1].as_str();
	let server_address = args[2].as_str();

	let app_data = AppData::initialize(index_path).unwrap();

	println!("Starting server at http://{}/", server_address);

	HttpServer::new(move || {
		App::new()
			.app_data(web::Data::new(app_data.clone()))
			.default_service(web::route().to(serve_default))
			.service(serve_search)
			.service(serve_stats)
	}).bind(server_address)?.run().await
}
