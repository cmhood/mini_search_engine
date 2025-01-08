import scrapy, json, os, sys, argparse
from scrapy.crawler import CrawlerProcess
from urllib.parse import urlparse, urljoin
from domains import domain_whitelists, domain_blacklists

def main():
	global whitelist
	global blacklist
	global output_dir

	if len(sys.argv) == 2 and sys.argv[1] == "--list-domains":
		print("\n".join(domain_whitelists.keys()))
		sys.exit(1)

	if len(sys.argv) != 3:
		print(f"usage: {sys.argv[0]} DOMAIN OUTPUT_DIR", file=sys.stderr)
		sys.exit(1)

	domain = sys.argv[1]
	output_dir = f"{sys.argv[2]}/{domain}"
	whitelist = domain_whitelists.get(domain, None)
	blacklist = domain_blacklists.get(domain, [])
	if whitelist is None:
		print(f"unsupported domain {domain}", file=sys.stderr)
		sys.exit(1)

	process = CrawlerProcess(settings={
		"LOG_LEVEL": "ERROR",
		"REDIRECT_ENABLED": False,
	})
	process.crawl(MiniSearchEngineSpider)
	process.start()

class MiniSearchEngineSpider(scrapy.Spider):
	name = "mini_search_engine_spider"

	def __init__(self):
		self.start_urls=whitelist
		self.allowed_domains=[urlparse(whitelist[0]).netloc]

	def parse(self, response):
		if self.get_absolute_url(response) is None:
			print(f"SKIP {response.url}: URL NOT IN WHITELIST")
			return

		content_type = response.headers.get("Content-Type", b"").decode("utf-8")
		if content_type.lower().replace(" ", "") not in ["text/html", "text/html;charset=utf-8"]:
			print(f"SKIP {response.url}: BAD CHARSET \"{content_type}\"")
			return

		links = set()
		for href in response.css("a::attr(href)").getall():
			url = self.get_absolute_url(response, href)
			if url is not None:
				links.add(url)

		if not self.download(response, links):
			print(f"SKIP {response.url}: NOT ENOUGH CONTENT")
			return

		print(response.url)

		for url in links:
			yield response.follow(url, self.parse)

	def download(self, response, links):
		data = {"url": response.url, "domain": urlparse(response.url).netloc, "links": list(links)}
		code_tags = "code tt pre kdb samp var".split(" ")
		heading_tags = "title h1 h2 h3 h4 h5 h6".split(" ")
		meta_selectors = [f"meta[name='{x}']::attr(content)" for x in ["keywords", "description"]]
		selectors = {
			"title": "title::text",
			"text": f"body *:not(script):not(style)::text",
			"headings": ", ".join([f"{t}::text, {t} *::text" for t in heading_tags] + meta_selectors),
			"code": ", ".join([f"{t}::text, {t} *::text" for t in code_tags]),
		}
		for key, sel in selectors.items():
			parts = [text.strip() for text in response.css(sel).getall()]
			data[key] = "\n".join(filter(None, parts))

		if len(data["text"]) < 200:
			return False

		os.makedirs(output_dir, exist_ok=True)
		filename = response.url.replace("/", "%2F")
		with open(f"{output_dir}/{filename}.json", "w") as f:
			json.dump(data, f)
		return True

	def get_absolute_url(self, response, url=""):
		# Basic check for special HTML hrefs
		if any(url.startswith(p) for p in ["#", "mailto:", "data:", "javascript:"]):
			return None
		# We still misinterpret other kinds of hrefs (e.g. other
		# protocols) but it doesn't really matter
		absolute_url = urljoin(response.url, url)
		if not any(absolute_url.startswith(w) for w in whitelist):
			return None
		if any(absolute_url.startswith(w) for w in blacklist):
			return None
		return absolute_url

if __name__ == "__main__":
	main()
