use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub async fn web_search(query: &str, max_results: usize) -> anyhow::Result<Vec<SearchResult>> {
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        urlencoding::encode(query)
    );

    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!("Search failed: {}", resp.status());
    }

    let html = resp.text().await?;
    let results = parse_search_results(&html, max_results);
    Ok(results)
}

fn parse_search_results(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    let title_re = regex::Regex::new(r#"<a[^>]*class="result__a"[^>]*>(.*?)</a>"#).unwrap();
    let snippet_re =
        regex::Regex::new(r#"<a[^>]*class="result__snippet"[^>]*>(.*?)</a>"#).unwrap();
    let url_re =
        regex::Regex::new(r#"<a[^>]*class="result__url"[^>]*>(.*?)</a>"#).unwrap();
    let strip_tags = regex::Regex::new(r"<[^>]+>").unwrap();

    let titles: Vec<String> = title_re
        .captures_iter(html)
        .map(|c| strip_tags.replace_all(c.get(1).unwrap().as_str(), "").trim().to_string())
        .collect();
    let snippets: Vec<String> = snippet_re
        .captures_iter(html)
        .map(|c| strip_tags.replace_all(c.get(1).unwrap().as_str(), "").trim().to_string())
        .collect();
    let urls: Vec<String> = url_re
        .captures_iter(html)
        .map(|c| strip_tags.replace_all(c.get(1).unwrap().as_str(), "").trim().to_string())
        .collect();

    let count = titles
        .len()
        .min(snippets.len())
        .min(urls.len())
        .min(max_results);
    for i in 0..count {
        results.push(SearchResult {
            title: titles[i].clone(),
            url: urls[i].clone(),
            snippet: snippets[i].clone(),
        });
    }

    results
}

#[cfg(test)]
mod rag_tests {
    use super::*;

    #[test]
    fn test_parse_empty_html() {
        let results = parse_search_results("", 10);
        assert!(results.is_empty());
    }

    #[test]
    fn test_parse_single_result() {
        let html = "<a class=\"result__a\" href=\"https://example.com\">Example Title</a>
        <a class=\"result__snippet\" href=\"https://example.com\">This is a snippet</a>
        <a class=\"result__url\" href=\"https://example.com\">example.com</a>";
        let results = parse_search_results(html, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example Title");
    }

    #[test]
    fn test_max_results_limit() {
        let html = "<a class=\"result__a\" href=\"#\">Title 1</a>
        <a class=\"result__snippet\" href=\"#\">Snippet 1</a>
        <a class=\"result__url\" href=\"https://a.com\">a.com</a>
        <a class=\"result__a\" href=\"#\">Title 2</a>
        <a class=\"result__snippet\" href=\"#\">Snippet 2</a>
        <a class=\"result__url\" href=\"https://b.com\">b.com</a>";
        let results = parse_search_results(html, 1);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_parse_with_bold_tags_in_snippet() {
        let html = "<a class=\"result__a\" href=\"https://example.com\">Example Title</a>
        <a class=\"result__snippet\" href=\"https://example.com\"><b>Test</b> your internet speed</a>
        <a class=\"result__url\" href=\"https://example.com\">example.com</a>";
        let results = parse_search_results(html, 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].snippet, "Test your internet speed");
    }
}
