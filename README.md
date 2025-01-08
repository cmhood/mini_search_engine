# Mini search engine

A mini search engine for viewing documentation from a selection of programming
documentation websites.

## Live demo

<https://search.chood.net/> (live as of 2025-01-08)

## Installation

Build the Rust binaries in `search/`:

```
cd search
cargo build --release
cd ..
```

Copy the binaries to `/usr/local/bin/` on the server where the search
engine will be hosted. If you are installing locally, this is done like so:

```
sudo cp search/target/release/indexer /usr/local/bin/mini-search-engine-indexer
sudo cp search/target/release/server /usr/local/bin/mini-search-engine-server
```

## Deployment

To set up the search engine, we must run the spider to scrape web content. Then,
we create an index from the pages. Finally, we can set up the search engine
application server to be accessible from the internet.

### Scraper

The scraper is a Python script using Scrapy. It is recommended to install
Scrapy in a virtual environment:

```
python -m venv venv
source venv/bin/activate
pip install -r requirements.txt
```

Execute the script at `scripts/scrape_all_domains.sh` to run the spider on every
supported domain. Any domains that have already been scraped (or partially
scraped) will be skipped, so the script can be paused and resumed. However, care
must be taken to ensure that websites which were only partially downloaded are
deleted and crawled again.

The script takes one argument, the path to the directory where downloaded pages
should be saved:

```
scripts/scrape_all_domains.sh spider_output
```

The script will create a directory called `spider_output` which contains a
subdirectory for each domain that is scraped. Crawling all domains will take
several hours.

The Python program found in `spider/` can also be called directly, but it only
allows you to scrape one domain at a time. Run `python3 spider` to see its
usage.

After running the spider, you can exit the virtual environment by running
`deactivate`.

### Indexer

Once the spider has crawled all domains, create an index from the output simply
by running the indexer:

```
mini-search-engine-indexer spider_output index
```

This will create an `index` directory containing the database and other files
needed by the search engine.

We can test the search engine by running `mini-search-engine-server`:

```
mini-search-engine-server index 127.0.0.1:8080
```

While the process is running, the search engine can be accessed locally at
<http://127.0.0.1:8080/>.

### System configuration

Create a user responsible for running the server as a daemon:

```
sudo useradd -r -s /usr/sbin/nologin -d /etc/mini_search_engine mini-search-engine
```

We will also move the index we created to a system directory:

```
sudo mkdir /var/lib/mini_search_engine
sudo mv index /var/lib/mini_search_engine
sudo chown -R mini-search-engine:mini-search-engine /var/lib/mini_search_engine
```

Next, we need an init script to run the `mini-search-engine-server` binary. On
systems using systemd, i.e. most Linux distributions, copy
`scripts/mini-search-engine.service` to `/etc/systemd/system/`. On other
systems, you will need to create your own init script.

The server can directly handle requests from the internet. However, it is
designed to run behind a reverse proxy. The reverse proxy should be configured
to forward requests to the address the server is listening to (`127.0.0.1:3000`
in the systemd service provided).

Using Nginx, we can use the following configuration, which can be set in
`/etc/nginx/nginx.conf`:

```
# ...
http {
	# ...
	server {
		listen 80 default_server;
		listen [::]:80 default_server;

		location / {
			proxy_pass http://127.0.0.1:3000$request_uri;
		}
	}
}
```

This will cause Nginx to route HTTP requests to the web server running on port
3000, where our search engine will be running when its systemd service is
enabled.

If you do not want to use a reverse proxy, you can change the port number in
the systemd service from 3000 to 80 (for HTTP). HTTPS is not supported without
a reverse proxy.

Next, reload the Nginx configuration and start the `mini-search-engine` service
we created:

```
sudo nginx -s reload
sudo systemctl start mini-search-engine
sudo systemctl enable mini-search-engine
```

The search engine is now reachable through the reverse proxy, making it
accessible from the internet. You can use the search engine by connecting to the
host from a web browser.
