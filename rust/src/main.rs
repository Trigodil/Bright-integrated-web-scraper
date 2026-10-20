mod client;
mod extract;
mod output;

use std::{collections::HashSet, fs, process::ExitCode};

use anyhow::Result;
use clap::{Parser, ValueEnum};
use futures::future::join_all;
use serde_json::{json, Value};

use client::{Fetcher, Options};
use extract::FieldSpec;

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Raw,
    Markdown,
}

/// Scrape web pages through Bright Data Web Unlocker.
#[derive(Parser)]
#[command(name = "brightscrape", version)]
struct Args {
    /// URLs to scrape
    urls: Vec<String>,
    /// File with one URL per line
    #[arg(short, long)]
    input: Option<String>,
    /// Field to extract as NAME=CSS[@ATTR] (repeatable)
    #[arg(short, long = "select")]
    select: Vec<String>,
    /// Repeating element; one record per match
    #[arg(long)]
    item: Option<String>,
    /// Next-page link to follow
    #[arg(long = "next")]
    next_sel: Option<String>,
    /// Pages to follow per start URL
    #[arg(long, default_value_t = 1)]
    max_pages: usize,
    #[arg(short, long, default_value_t = 10)]
    concurrency: usize,
    #[arg(short, long, default_value_t = 3)]
    retries: usize,
    /// Per-request timeout in seconds
    #[arg(long, default_value_t = 90.0)]
    timeout: f64,
    #[arg(long, value_enum, default_value = "raw")]
    format: Format,
    /// Two-letter country code for geo-targeting
    #[arg(long)]
    country: Option<String>,
    /// Fetch directly, bypassing Bright Data
    #[arg(long)]
    direct: bool,
    /// Output file (.json, .jsonl, .csv); stdout if omitted
    #[arg(short, long)]
    output: Option<String>,
    #[arg(short, long)]
    verbose: bool,
}

fn load_urls(args: &Args) -> Result<Vec<String>> {
    let mut urls = args.urls.clone();
    if let Some(path) = &args.input {
        urls.extend(
            fs::read_to_string(path)?
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(String::from),
        );
    }
    let mut seen = HashSet::new();
    urls.retain(|u| seen.insert(u.clone()));
    Ok(urls)
}

async fn crawl(fetcher: &Fetcher, start: &str, args: &Args, fields: &[FieldSpec]) -> Result<Vec<Value>> {
    let (mut records, mut seen) = (Vec::new(), HashSet::new());
    let mut url = Some(start.to_string());
    for _ in 0..args.max_pages.max(1) {
        let Some(cur) = url.take().filter(|u| seen.insert(u.clone())) else { break };
        let res = fetcher.fetch(&cur).await?;
        if fields.is_empty() {
            if args.verbose {
                eprintln!("INFO {cur} -> {} bytes", res.body.len());
            }
            records.push(json!({ "url": cur, "status": res.status, "content": res.body }));
        } else {
            let page = extract::extract(&res.body, &cur, fields, args.item.as_deref())?;
            if args.verbose {
                eprintln!("INFO {cur} -> {} record(s)", page.len());
            }
            records.extend(page);
        }
        url = match &args.next_sel {
            Some(css) => extract::find_next(&res.body, &cur, css)?,
            None => None,
        };
    }
    Ok(records)
}

async fn run(args: Args) -> Result<u8> {
    let urls = load_urls(&args)?;
    if urls.is_empty() {
        anyhow::bail!("No URLs given");
    }
    let fields = args.select.iter().map(|s| FieldSpec::parse(s)).collect::<Result<Vec<_>>>()?;
    for css in args.item.iter().chain(&args.next_sel) {
        extract::validate(css)?;
    }
    let fetcher = Fetcher::new(Options {
        direct: args.direct,
        markdown: matches!(args.format, Format::Markdown),
        country: args.country.clone(),
        concurrency: args.concurrency,
        retries: args.retries,
        timeout_secs: args.timeout,
        ..Default::default()
    })?;
    let results = join_all(urls.iter().map(|u| crawl(&fetcher, u, &args, &fields))).await;
    let (mut records, mut failed) = (Vec::new(), 0);
    for (u, r) in urls.iter().zip(results) {
        match r {
            Ok(recs) => records.extend(recs),
            Err(e) => {
                failed += 1;
                eprintln!("ERROR Failed {u}: {e:#}");
            }
        }
    }
    output::write_records(&records, args.output.as_deref())?;
    if args.verbose {
        eprintln!("INFO Done: {} record(s), {failed} failed URL(s)", records.len());
    }
    Ok(if failed > 0 && records.is_empty() { 1 } else { 0 })
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Args::parse()).await {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("ERROR {e:#}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener};

    // Minimal HTTP server that answers with the given statuses in order, then repeats the last.
    async fn serve(statuses: Vec<u16>) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            for i in 0.. {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = vec![0u8; 8192];
                let n = sock.read(&mut buf).await.unwrap();
                tx.send(String::from_utf8_lossy(&buf[..n]).into_owned()).ok();
                let status = *statuses.get(i).or(statuses.last()).unwrap();
                let body = "<h1>ok</h1>";
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                sock.write_all(resp.as_bytes()).await.unwrap();
            }
        });
        (format!("http://{addr}/request"), rx)
    }

    fn opts(api_url: String) -> Options {
        Options {
            retries: 3,
            concurrency: 2,
            timeout_secs: 10.0,
            api_url: Some(api_url),
            api_key: Some("key".into()),
            zone: Some("zone1".into()),
            country: Some("us".into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn unlocker_payload_and_retry() {
        let (api, mut rx) = serve(vec![502, 200]).await;
        let res = Fetcher::new(opts(api)).unwrap().fetch("https://target.test/").await.unwrap();
        assert_eq!(res.body, "<h1>ok</h1>");
        rx.recv().await.unwrap();
        let req = rx.recv().await.unwrap();
        assert!(req.to_lowercase().contains("authorization: bearer key"));
        assert!(req.contains(r#""zone":"zone1""#) && req.contains(r#""country":"us""#));
    }

    #[tokio::test]
    async fn client_error_is_not_retried() {
        let (api, mut rx) = serve(vec![401]).await;
        let err = Fetcher::new(opts(api)).unwrap().fetch("https://t.test/").await.unwrap_err();
        assert!(err.to_string().contains("401"));
        rx.recv().await.unwrap();
        assert!(rx.try_recv().is_err());
    }
}
