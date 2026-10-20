import asyncio
import json

import pytest
from aiohttp import web

from brightscrape import FetchError, Fetcher, FieldSpec, extract, find_next
from brightscrape.cli import main

LIST_HTML = """
<ul>
  <li class="card"><a href="/p/1">One</a><span class="price">$1</span></li>
  <li class="card"><a href="/p/2">Two</a></li>
</ul>
<a rel="next" href="/page/2">next</a>
"""


def test_field_spec_parse():
    assert FieldSpec.parse("title=h1") == FieldSpec("title", "h1", None)
    assert FieldSpec.parse("link=a.x@href") == FieldSpec("link", "a.x", "href")
    with pytest.raises(ValueError):
        FieldSpec.parse("nope")


def test_extract_items_and_links():
    fields = [FieldSpec.parse("name=a"), FieldSpec.parse("link=a@href"),
              FieldSpec.parse("price=.price")]
    recs = extract(LIST_HTML, "https://x.test/list", fields, ".card")
    assert recs[0] == {"url": "https://x.test/list", "name": "One",
                       "link": "https://x.test/p/1", "price": "$1"}
    assert recs[1]["price"] is None
    assert find_next(LIST_HTML, "https://x.test/list", "a[rel=next]") == "https://x.test/page/2"


def run_with_server(handler, coro_fn):
    async def go():
        app = web.Application()
        app.router.add_route("*", "/{tail:.*}", handler)
        runner = web.AppRunner(app)
        await runner.setup()
        site = web.TCPSite(runner, "127.0.0.1", 0)
        await site.start()
        port = site._server.sockets[0].getsockname()[1]
        try:
            return await coro_fn(f"http://127.0.0.1:{port}")
        finally:
            await runner.cleanup()

    return asyncio.run(go())


def test_unlocker_payload_and_retry():
    calls = []

    async def handler(req):
        calls.append((req.headers.get("Authorization"), await req.json()))
        if len(calls) == 1:
            return web.Response(status=502)
        return web.Response(text="<h1>ok</h1>")

    async def go(base):
        async with Fetcher("key", "zone1", api_url=f"{base}/request", country="us",
                           retries=3) as f:
            return await f.fetch("https://target.test/")

    res = run_with_server(handler, go)
    assert res.body == "<h1>ok</h1>"
    assert len(calls) == 2
    assert calls[1] == ("Bearer key", {"zone": "zone1", "url": "https://target.test/",
                                       "format": "raw", "country": "us"})


def test_client_error_is_not_retried():
    calls = []

    async def handler(req):
        calls.append(1)
        return web.Response(status=401, text="bad key")

    async def go(base):
        async with Fetcher("key", "zone", api_url=f"{base}/request", retries=3) as f:
            await f.fetch("https://target.test/")

    with pytest.raises(FetchError, match="401"):
        run_with_server(handler, go)
    assert len(calls) == 1


def test_missing_credentials(monkeypatch):
    monkeypatch.delenv("BRIGHTDATA_API_KEY", raising=False)
    monkeypatch.delenv("BRIGHTDATA_UNLOCKER_ZONE", raising=False)
    with pytest.raises(FetchError):
        Fetcher()


def test_cli_direct_pagination(tmp_path):
    async def handler(req):
        if req.path == "/page/2":
            return web.Response(text='<li class="card"><a href="/p/3">Three</a></li>',
                                content_type="text/html")
        return web.Response(text=LIST_HTML, content_type="text/html")

    out = tmp_path / "out.json"

    async def go(base):
        args = [f"{base}/list", "--direct", "--item", ".card", "-s", "name=a",
                "--next", "a[rel=next]", "--max-pages", "5", "-o", str(out)]
        return await asyncio.to_thread(main, args)

    assert run_with_server(handler, go) == 0
    assert [r["name"] for r in json.loads(out.read_text())] == ["One", "Two", "Three"]
