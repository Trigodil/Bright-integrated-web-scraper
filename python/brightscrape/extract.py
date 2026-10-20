from dataclasses import dataclass
from urllib.parse import urljoin

from bs4 import BeautifulSoup, Tag


@dataclass
class FieldSpec:
    """A named field: CSS selector plus optional attribute, written as `name=css` or `name=css@attr`."""

    name: str
    selector: str
    attr: str | None = None

    @classmethod
    def parse(cls, spec: str) -> "FieldSpec":
        name, sep, rest = spec.partition("=")
        if not sep or not name.strip() or not rest.strip():
            raise ValueError(f"Invalid field spec {spec!r}, expected name=css or name=css@attr")
        selector, at, attr = rest.rpartition("@")
        if not at:
            selector, attr = rest, None
        return cls(name.strip(), selector.strip(), (attr or "").strip() or None)


def _value(root: Tag, field: FieldSpec, base_url: str) -> str | None:
    el = root.select_one(field.selector) if field.selector else root
    if el is None:
        return None
    if field.attr is None:
        return el.get_text(" ", strip=True)
    val = el.get(field.attr)
    if isinstance(val, list):
        val = " ".join(val)
    if val is not None and field.attr in ("href", "src"):
        val = urljoin(base_url, val)
    return val


def extract(html: str, url: str, fields: list[FieldSpec], item: str | None = None) -> list[dict]:
    """Returns one record per `item` match, or a single page-level record."""
    soup = BeautifulSoup(html, "html.parser")
    roots = soup.select(item) if item else [soup]
    return [{"url": url, **{f.name: _value(r, f, url) for f in fields}} for r in roots]


def find_next(html: str, url: str, selector: str) -> str | None:
    el = BeautifulSoup(html, "html.parser").select_one(selector)
    href = el.get("href") if el else None
    return urljoin(url, href) if href else None
