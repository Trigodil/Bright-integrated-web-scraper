use std::{env, sync::Arc, time::Duration};

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use tokio::sync::Semaphore;

pub const DEFAULT_API_URL: &str = "https://api.brightdata.com/request";
const RETRY_STATUSES: [u16; 5] = [429, 500, 502, 503, 504];

#[derive(Debug, Clone, Default)]
pub struct Options {
    pub direct: bool,
    pub markdown: bool,
    pub country: Option<String>,
    pub concurrency: usize,
    pub retries: usize,
    pub timeout_secs: f64,
    pub api_url: Option<String>,
    pub api_key: Option<String>,
    pub zone: Option<String>,
}

#[derive(Debug)]
pub struct FetchResult {
    pub status: u16,
    pub body: String,
}

/// Fetches pages through Bright Data Web Unlocker, or directly when `direct` is set.
#[derive(Clone)]
pub struct Fetcher {
    http: reqwest::Client,
    sem: Arc<Semaphore>,
    opts: Options,
    api_url: String,
    key: String,
    zone: String,
}

impl Fetcher {
    pub fn new(opts: Options) -> Result<Self> {
        let key = opts.api_key.clone().or_else(|| env::var("BRIGHTDATA_API_KEY").ok()).unwrap_or_default();
        let zone = opts.zone.clone().or_else(|| env::var("BRIGHTDATA_UNLOCKER_ZONE").ok()).unwrap_or_default();
        if !opts.direct && (key.is_empty() || zone.is_empty()) {
            bail!("Set BRIGHTDATA_API_KEY and BRIGHTDATA_UNLOCKER_ZONE, or use --direct");
        }
        let api_url = opts.api_url.clone()
            .or_else(|| env::var("BRIGHTDATA_API_URL").ok())
            .unwrap_or_else(|| DEFAULT_API_URL.into());
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs_f64(opts.timeout_secs.max(1.0)))
            .user_agent("brightscrape/0.1")
            .build()?;
        let sem = Arc::new(Semaphore::new(opts.concurrency.max(1)));
        Ok(Self { http, sem, opts, api_url, key, zone })
    }

    pub fn payload(&self, url: &str) -> Value {
        let mut p = json!({ "zone": self.zone, "url": url, "format": "raw" });
        if self.opts.markdown {
            p["data_format"] = "markdown".into();
        }
        if let Some(c) = &self.opts.country {
            p["country"] = c.as_str().into();
        }
        p
    }

    fn request(&self, url: &str) -> reqwest::RequestBuilder {
        if self.opts.direct {
            self.http.get(url)
        } else {
            self.http.post(&self.api_url).bearer_auth(&self.key).json(&self.payload(url))
        }
    }

    pub async fn fetch(&self, url: &str) -> Result<FetchResult> {
        let _permit = self.sem.acquire().await?;
        let attempts = self.opts.retries.max(1);
        let mut last = anyhow!("no attempts made");
        for attempt in 0..attempts {
            let retryable = match self.request(url).send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let body = resp.text().await.unwrap_or_default();
                    if status < 400 {
                        return Ok(FetchResult { status, body });
                    }
                    let snippet: String = body.chars().take(200).collect();
                    last = anyhow!("HTTP {status}: {snippet}");
                    RETRY_STATUSES.contains(&status)
                }
                Err(e) => {
                    last = e.into();
                    true
                }
            };
            if !retryable {
                break;
            }
            if attempt + 1 < attempts {
                let wait = 1u64 << attempt;
                eprintln!("WARN {url} failed ({last}), retrying in {wait}s");
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
        }
        Err(anyhow!("{url}: {last}"))
    }
}
