# Bright-integrated-web-scraper
Open source web scraper powered by [Bright Data Web Unlocker](https://docs.brightdata.com/scraping-automation/web-unlocker/introduction), in two flavours with an identical CLI:

- **`python/`**: asyncio + aiohttp + BeautifulSoup
- **`rust/`**: tokio + reqwest + scraper

Features: CSS-selector extraction, repeating-item records, next-link pagination, concurrent fetching with a semaphore, retries with exponential backoff, geo-targeting, and JSON / JSONL / CSV output.

## Setup

```bash
export BRIGHTDATA_API_KEY="your-api-key"        # https://brightdata.com/cp/setting/users
export BRIGHTDATA_UNLOCKER_ZONE="your-zone-name"
```

```bash
# Python
pip install ./python
brightscrape --help

# Rust
cargo build --release --manifest-path rust/Cargo.toml
./rust/target/release/brightscrape --help
```

## Usage

```bash
# Raw page content
brightscrape https://example.com

# One record per quote, following "next" links for up to 3 pages, saved as CSV
brightscrape https://quotes.toscrape.com/ \
  --item .quote \
  -s "text=.text" -s "author=.author" -s "about=a[href^='/author']@href" \
  --next "li.next a" --max-pages 3 -o quotes.csv

# Many URLs from a file, 20 at a time, geo-targeted to Germany
brightscrape -i urls.txt -s "title=h1" -c 20 --country de -o out.jsonl
```

| Flag | Meaning |
|------|---------|
| `-s NAME=CSS[@ATTR]` | Field to extract (repeatable). `href`/`src` attributes become absolute URLs. |
| `--item CSS` | Repeating element; emits one record per match. |
| `--next CSS` / `--max-pages N` | Follow a next-page link up to N pages per start URL. |
| `-i FILE` | Read URLs from a file, one per line (`#` comments allowed). |
| `-c N` / `-r N` / `--timeout S` | Concurrency, retry attempts, per-request timeout. |
| `--format raw\|markdown` | Ask Web Unlocker for raw HTML or markdown. |
| `--country CC` | Geo-target the request. |
| `--direct` | Bypass Bright Data and fetch directly (handy for local testing). |
| `-o FILE` | `.json`, `.jsonl` or `.csv`; JSON to stdout if omitted. |

Set `BRIGHTDATA_API_URL` to point at a different endpoint (used by the tests).

## Tests

```bash
cd python && pip install -e '.[dev]' && pytest
cd rust && cargo test
```
