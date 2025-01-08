# Writeup - Creating a mini search engine

## Crawling

I looked through the different options and it seems like pretty much everyone
uses Scrapy, so that's what I went with.

Some websites were SPAs that rendered themselves on the client side, so I could
not crawl those since I didn't want to have to run JavaScript.

Other websites were organized into very long pages with links to different
sections within the page. I would have had to have written custom logic for
each of these websites in order to turn each section into essentially its own
web page. But since only a few websites did this, I decided against this and
simply skipped these domains.

Some websites, like `www.php.net`, detected that I was crawling the website and
forced me to solve CAPTCHAs after every few hundred pages. Other websites were
similarly hard to download. This made it difficult to automate the crawler.

I originally wanted to design the search engine so that it would automatically
re-crawl and reindex websites at a regular interval (weekly, e.g.). This is
actually trivial to set up with my current code -- just create a cron job
to run `scripts/scrape_all_domains.sh`, then regenerate the index, then restart
the server process.

But since there are so many issues that can come up during crawling, I do not
think this would work reliably in practice. I would instead have to cache
webpages I had crawled before, in case pages were temporarily inaccessible.
If I wanted to do this efficiently, I would be dealing with new issues relating
to reading from the index while it is being updated.

If I were writing a general-purpose search engine, it would be much more
important to auto-update the index, but since I am already manually choosing
which specific domains to crawl, and which pages on each of those domains,
I do not consider it as important to be able to update the index automatically.

## Indexing

Between Tantivy and Vespa, I chose the former. Before working on this project,
I was not familiar with either one.

Tantivy pros:

  * Specifically designed for search engines and nothing else
  * Touts very low latency in benchmarks
  * Much simpler API
  * Easy to deploy -- just a statically linked library

Tantivy cons:

  * API available in Rust with limited bindings to other languages

Vespa pros:

  * More advanced NLP

Vespa cons:

  * Cites "less than 100 milliseconds" as a typical latency requirement, but
    our maximum latency is 50 ms
  * Complicated deployment
  * Lots of features I don't need which could make things I do want to do harder

Since Tantivy is written in Rust, and most of the code I wrote would be
interfacing with it, I decided to write the entire server in Rust. This was
fine because Rust has a good selection of web app frameworks. I also prefer
the strong typing of Rust compared to Python or Ruby (the two language with
official bindings).

One major problem I had with Tantivy is that its tokenizer is apparently very
slow. When benchmarking my application, I noticed that about 90% of the time
spent handling each request was spent tokenizing the results in order to display
the snippets.

I don't think tokenization should be that slow, but it also looks like Tantivy's
public interface for tokenizing text is not designed with performance in mind.
It appears to be much faster at performing tokenization internally.

I had three options:

 1. Don't use Tantivy's tokenizer API and write my own tokenizer. Tantivy
    supports pre-tokenized text.
 2. Store the tokenized text in a separate database (or possibly in the
    Tantivy database). Tantivy doesn't support storing tokens, so I would have
    to do my own serialization.
 3. Just don't tokenize the snippets in the search results.

I went with the third option because it is the simplest and I was the most
confident that it would have good performance. I use a simple text replacement
to embolden search terms in the snippets shown for each search result.

If I spent more time on the search engine, I would definitely want to fix the
tokenization of the search results. However, the search engine works fine this
way, and my application is still able to show relevant snippets of the webpages,
so in terms of cost-benefit analysis, this was a more reasonable place to cut
corners.

## Ranking

There was not much information available for me to rank the pages. I started
by working with Tantivy's default ranking algorithm. I then made it so that
text in titles and headings would be weighted more heavily than text in
paragraphs or other parts of pages.

I also implemented the PageRank algorithm on domain individual. This gives each
page a score, which I factor into the final ranking of each page.

PageRank is more relevant for more general search queries and less relevant
when the user provides lots of search terms. Tantivy natually give higher scores
to relevant pages when you use lots of search terms, since there are more
matching keywords. Based on my own informal testing, I found what I considered
the best way to combine the page rank score with the rating Tantivy gives from
matching keywords.

My search engine also allows users to use special syntax to specify which
specific domain they want to search, specific phrases they want to find (rather
than just sets of keywords), and specific pieces of code they want to find in
the page. Using these advanced features can give much better search results than
searching for sets of keywords.

During testing, I used a much smaller set of domains, and I was worried that
when I tested with more domains, I would have an issue where a small set of
domains showed up in the results for any given search. Instead, I have the
opposite problem: too many irrelevant pages from irrelevant domains show up in
the results.

PageRank fails to mitigate this because there are very few links between the
websites I index, and my implementation only considers internal links for each
domain anyway.

My search engine's greatest weakness is how it ranks the results from searches
with few keywords. There are some options I could try to get better results.

I think if I penalized pages that contained uncommon, irrelevant keywords,
the search results would be better. However, I could not find a way to do this
without significantly hurting performance. I wanted to do it in two passes:
one performing the search normally, and then a second pass that finds results
that should be penalized because they match tokens we don't like.

The PageRank algorithm also helps with ranking results when there are a lot of
results that the index can't differentiate between.
