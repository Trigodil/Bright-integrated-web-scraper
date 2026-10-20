use anyhow::{anyhow, bail, Result};
use scraper::{ElementRef, Html, Selector};
use serde_json::{Map, Value};
use url::Url;

/// A named field: CSS selector plus optional attribute, written as `name=css` or `name=css@attr`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    pub name: String,
    pub selector: String,
    pub attr: Option<String>,
}

impl FieldSpec {
    pub fn parse(spec: &str) -> Result<Self> {
        let (name, rest) = spec
            .split_once('=')
            .filter(|(n, r)| !n.trim().is_empty() && !r.trim().is_empty())
            .ok_or_else(|| anyhow!("Invalid field spec {spec:?}, expected name=css or name=css@attr"))?;
        let (selector, attr) = match rest.rsplit_once('@') {
            Some((s, a)) if !a.trim().is_empty() => (s, Some(a.trim().to_string())),
            _ => (rest, None),
        };
        Ok(Self { name: name.trim().into(), selector: selector.trim().into(), attr })
    }
}

fn selector(css: &str) -> Result<Selector> {
    Selector::parse(css).map_err(|e| anyhow!("Bad selector {css:?}: {e}"))
}

fn join(base: &str, href: &str) -> String {
    Url::parse(base).and_then(|b| b.join(href)).map(|u| u.to_string()).unwrap_or_else(|_| href.into())
}

fn value(root: ElementRef, sel: &Option<Selector>, field: &FieldSpec, base: &str) -> Value {
    let el = match sel {
        Some(s) => match root.select(s).next() {
            Some(e) => e,
            None => return Value::Null,
        },
        None => root,
    };
    match &field.attr {
        None => Value::String(el.text().map(str::trim).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ")),
        Some(a) => match el.value().attr(a) {
            Some(v) if a == "href" || a == "src" => Value::String(join(base, v)),
            Some(v) => Value::String(v.into()),
            None => Value::Null,
        },
    }
}

/// Returns one record per `item` match, or a single page-level record.
pub fn extract(html: &str, url: &str, fields: &[FieldSpec], item: Option<&str>) -> Result<Vec<Value>> {
    let doc = Html::parse_document(html);
    let sels = fields
        .iter()
        .map(|f| if f.selector.is_empty() { Ok(None) } else { selector(&f.selector).map(Some) })
        .collect::<Result<Vec<_>>>()?;
    let roots: Vec<ElementRef> = match item {
        Some(css) => doc.select(&selector(css)?).collect(),
        None => vec![doc.root_element()],
    };
    Ok(roots
        .into_iter()
        .map(|root| {
            let mut rec = Map::new();
            rec.insert("url".into(), url.into());
            for (f, s) in fields.iter().zip(&sels) {
                rec.insert(f.name.clone(), value(root, s, f, url));
            }
            Value::Object(rec)
        })
        .collect())
}

pub fn find_next(html: &str, url: &str, css: &str) -> Result<Option<String>> {
    let doc = Html::parse_document(html);
    let sel = selector(css)?;
    Ok(doc.select(&sel).next().and_then(|e| e.value().attr("href")).map(|h| join(url, h)))
}

pub fn validate(css: &str) -> Result<()> {
    if css.trim().is_empty() {
        bail!("Empty selector");
    }
    selector(css).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"<ul>
      <li class="card"><a href="/p/1">One</a><span class="price">$1</span></li>
      <li class="card"><a href="/p/2">Two</a></li></ul>
      <a rel="next" href="/page/2">next</a>"#;

    #[test]
    fn parses_specs() {
        assert_eq!(FieldSpec::parse("link=a.x@href").unwrap().attr.as_deref(), Some("href"));
        assert_eq!(FieldSpec::parse("t=h1").unwrap().attr, None);
        assert!(FieldSpec::parse("nope").is_err());
    }

    #[test]
    fn extracts_items_and_next() {
        let fields: Vec<_> = ["name=a", "link=a@href", "price=.price"]
            .iter().map(|s| FieldSpec::parse(s).unwrap()).collect();
        let recs = extract(LIST, "https://x.test/list", &fields, Some(".card")).unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0]["link"], "https://x.test/p/1");
        assert_eq!(recs[0]["price"], "$1");
        assert!(recs[1]["price"].is_null());
        let next = find_next(LIST, "https://x.test/list", "a[rel=next]").unwrap();
        assert_eq!(next.as_deref(), Some("https://x.test/page/2"));
    }
}
