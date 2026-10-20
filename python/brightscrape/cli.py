import argparse
import asyncio
import logging
import sys

from .client import Fetcher, FetchError
from .extract import FieldSpec, extract, find_next
from .output import write_records

log = logging.getLogger("brightscrape")


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="brightscrape", description="Scrape web pages through Bright Data Web Unlocker."
    )
    p.add_argument("urls", nargs="*", help="URLs to scrape")
    p.add_argument("-i", "--input", help="File with one URL per line")
    p.add_argument("-s", "--select", action="append", default=[], metavar="NAME=CSS[@ATTR]",
                   help="Field to extract (repeatable)")
    p.add_argument("--item", metavar="CSS", help="Repeating element; one record per match")
    p.add_argument("--next", dest="next_sel", metavar="CSS", help="Next-page link to follow")
    p.add_argument("--max-pages", type=int, default=1, help="Pages to follow per start URL")
    p.add_argument("-c", "--concurrency", type=int, default=10)
    p.add_argument("-r", "--retries", type=int, default=3)
    p.add_argument("--timeout", type=float, default=90.0, help="Per-request timeout in seconds")
    p.add_argument("--format", choices=["raw", "markdown"], default="raw", dest="data_format")
    p.add_argument("--country", help="Two-letter country code for geo-targeting")
    p.add_argument("--direct", action="store_true", help="Fetch directly, bypassing Bright Data")
    p.add_argument("-o", "--output", help="Output file (.json, .jsonl, .csv); stdout if omitted")
    p.add_argument("-v", "--verbose", action="store_true")
    return p


def load_urls(args) -> list[str]:
    urls = list(args.urls)
    if args.input:
        with open(args.input, encoding="utf-8") as f:
            urls += [ln.strip() for ln in f if ln.strip() and not ln.startswith("#")]
    return list(dict.fromkeys(urls))


async def crawl(fetcher: Fetcher, start: str, args, fields: list[FieldSpec]) -> list[dict]:
    records, url, seen = [], start, set()
    for _ in range(max(1, args.max_pages)):
        if url in seen:
            break
        seen.add(url)
        res = await fetcher.fetch(url)
        if fields:
            page = extract(res.body, url, fields, args.item)
            records.extend(page)
            log.info("%s -> %d record(s)", url, len(page))
        else:
            records.append({"url": url, "status": res.status, "content": res.body})
            log.info("%s -> %d bytes", url, len(res.body))
        url = find_next(res.body, url, args.next_sel) if args.next_sel else None
        if not url:
            break
    return records


async def run(args) -> int:
    urls = load_urls(args)
    if not urls:
        log.error("No URLs given")
        return 2
    fields = [FieldSpec.parse(s) for s in args.select]
    async with Fetcher(direct=args.direct, data_format=args.data_format, country=args.country,
                       concurrency=args.concurrency, retries=args.retries,
                       timeout=args.timeout) as fetcher:
        results = await asyncio.gather(*(crawl(fetcher, u, args, fields) for u in urls),
                                       return_exceptions=True)
    records, failed = [], 0
    for u, r in zip(urls, results):
        if isinstance(r, Exception):
            failed += 1
            log.error("Failed %s: %s", u, r)
        else:
            records.extend(r)
    write_records(records, args.output)
    log.info("Done: %d record(s), %d failed URL(s)", len(records), failed)
    return 1 if failed and not records else 0


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    logging.basicConfig(level=logging.INFO if args.verbose else logging.WARNING,
                        format="%(levelname)s %(message)s", stream=sys.stderr)
    try:
        return asyncio.run(run(args))
    except (FetchError, ValueError) as e:
        log.error("%s", e)
        return 2


if __name__ == "__main__":
    sys.exit(main())
