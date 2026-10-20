"""Open source web scraper powered by Bright Data Web Unlocker."""

from .client import Fetcher, FetchError
from .extract import FieldSpec, extract, find_next

__all__ = ["Fetcher", "FetchError", "FieldSpec", "extract", "find_next"]
__version__ = "0.1.0"
