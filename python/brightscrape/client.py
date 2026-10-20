import asyncio
import logging
import os
from dataclasses import dataclass

import aiohttp

log = logging.getLogger("brightscrape")

DEFAULT_API_URL = "https://api.brightdata.com/request"
RETRY_STATUSES = {429, 500, 502, 503, 504}


class FetchError(Exception):
    def __init__(self, msg: str, retryable: bool = False):
        super().__init__(msg)
        self.retryable = retryable


@dataclass
class FetchResult:
    url: str
    status: int
    body: str


class Fetcher:
    """Fetches pages through Bright Data Web Unlocker, or directly when `direct` is set."""

    def __init__(
        self,
        api_key: str | None = None,
        zone: str | None = None,
        *,
        direct: bool = False,
        data_format: str = "raw",
        country: str | None = None,
        concurrency: int = 10,
        retries: int = 3,
        timeout: float = 90.0,
        api_url: str | None = None,
    ):
        self.direct = direct
        self.api_key = api_key or os.environ.get("BRIGHTDATA_API_KEY")
        self.zone = zone or os.environ.get("BRIGHTDATA_UNLOCKER_ZONE")
        self.api_url = api_url or os.environ.get("BRIGHTDATA_API_URL", DEFAULT_API_URL)
        if not direct and not (self.api_key and self.zone):
            raise FetchError("Set BRIGHTDATA_API_KEY and BRIGHTDATA_UNLOCKER_ZONE, or use --direct")
        self.data_format = data_format
        self.country = country
        self.retries = max(1, retries)
        self.timeout = aiohttp.ClientTimeout(total=timeout)
        self._sem = asyncio.Semaphore(max(1, concurrency))
        self._session: aiohttp.ClientSession | None = None

    async def __aenter__(self):
        self._session = aiohttp.ClientSession(timeout=self.timeout)
        return self

    async def __aexit__(self, *exc):
        if self._session:
            await self._session.close()

    def _request_args(self, url: str) -> dict:
        if self.direct:
            return {"method": "GET", "url": url, "headers": {"User-Agent": "brightscrape/0.1"}}
        payload = {"zone": self.zone, "url": url, "format": "raw"}
        if self.data_format == "markdown":
            payload["data_format"] = "markdown"
        if self.country:
            payload["country"] = self.country
        return {
            "method": "POST",
            "url": self.api_url,
            "json": payload,
            "headers": {"Authorization": f"Bearer {self.api_key}"},
        }

    async def fetch(self, url: str) -> FetchResult:
        assert self._session, "Fetcher must be used as an async context manager"
        last_err: Exception | None = None
        async with self._sem:
            for attempt in range(self.retries):
                try:
                    async with self._session.request(**self._request_args(url)) as resp:
                        body = await resp.text()
                        if resp.status >= 400:
                            # Client errors such as a bad key or zone stay failed on retry.
                            raise FetchError(f"HTTP {resp.status}: {body[:200]}",
                                             retryable=resp.status in RETRY_STATUSES)
                        return FetchResult(url, resp.status, body)
                except (aiohttp.ClientError, asyncio.TimeoutError, FetchError) as e:
                    last_err = e
                    if isinstance(e, FetchError) and not e.retryable:
                        break
                    if attempt < self.retries - 1:
                        wait = 2**attempt
                        log.warning("%s failed (%s), retrying in %ss", url, e, wait)
                        await asyncio.sleep(wait)
        raise FetchError(f"{url}: {last_err}")
