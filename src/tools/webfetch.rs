//! `WebFetch` tool — mirrors `src/tools/WebFetchTool/WebFetchTool.ts`.

use crate::types::{
    AppError, FetchTimeoutSecs, MaxHttpContentLength, MaxMarkdownLength, MaxUrlLength,
    ToolDefinition, ToolOutput, ToolResultStatus, WebFetchInput,
};

pub(crate) fn webfetch_definition() -> ToolDefinition {
    ToolDefinition {
        name: "WebFetch".into(),
        description: "Fetches content from a URL and extracts information.".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "The URL to fetch content from" },
                "prompt": { "type": "string", "description": "What information to extract from the page" }
            },
            "required": ["url", "prompt"]
        }),
    }
}

pub(crate) async fn execute_webfetch(input: serde_json::Value) -> (ToolOutput, ToolResultStatus) {
    let parsed: WebFetchInput = match serde_json::from_value(input) {
        Ok(v) => v,
        Err(e) => {
            return (
                ToolOutput::new(format!("Invalid WebFetch input: {e}")),
                ToolResultStatus::Error,
            );
        }
    };

    if let Err(e) = validate_fetch_url(parsed.url.as_ref()) {
        return (ToolOutput::new(e.to_string()), ToolResultStatus::Error);
    }

    let fetch_url = upgrade_to_https(parsed.url.as_ref());
    let start = std::time::Instant::now();

    match fetch_with_redirect_policy(&fetch_url).await {
        Ok(FetchResult::CrossHostRedirect {
            original,
            redirect,
            status,
        }) => {
            let msg = format!(
                "REDIRECT DETECTED: The URL redirects to a different host.\n\n\
                 Original URL: {original}\nRedirect URL: {redirect}\nStatus: {status}\n\n\
                 To complete your request, use WebFetch again with:\n\
                 - url: \"{redirect}\"\n- prompt: \"{}\"",
                parsed.prompt.as_ref()
            );
            (ToolOutput::new(msg), ToolResultStatus::Success)
        }
        Ok(FetchResult::Success(response)) => {
            build_fetch_output(response, &fetch_url, &parsed, start).await
        }
        Err(e) => (
            ToolOutput::new(format!("Fetch failed: {e}")),
            ToolResultStatus::Error,
        ),
    }
}

/// Build the final output from a successful HTTP response.
async fn build_fetch_output(
    response: reqwest::Response,
    fetch_url: &str,
    parsed: &WebFetchInput,
    start: std::time::Instant,
) -> (ToolOutput, ToolResultStatus) {
    let status = response.status();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (
                ToolOutput::new(format!("Error reading response body: {e}")),
                ToolResultStatus::Error,
            );
        }
    };

    if bytes.len() as u64 > MaxHttpContentLength::DEFAULT.value() {
        return (
            ToolOutput::new(format!(
                "Response too large ({} bytes, max {} bytes)",
                bytes.len(),
                MaxHttpContentLength::DEFAULT.value()
            )),
            ToolResultStatus::Error,
        );
    }

    let raw_text = String::from_utf8_lossy(&bytes);
    let markdown = if content_type.contains("text/html") {
        htmd::convert(&raw_text).unwrap_or_else(|_| raw_text.into_owned())
    } else {
        raw_text.into_owned()
    };

    let max_len = MaxMarkdownLength::DEFAULT.value();
    let content = if markdown.len() > max_len {
        format!(
            "{}\n\n[Content truncated ({} chars, max {max_len})]",
            &markdown[..max_len],
            markdown.len()
        )
    } else {
        markdown
    };

    let elapsed = start.elapsed();
    let result = format!(
        "URL: {fetch_url}\nStatus: {} {}\nBytes: {}\nDuration: {}ms\n\
         Prompt: {}\n\n---\n\n{content}",
        status.as_u16(),
        status.canonical_reason().unwrap_or(""),
        bytes.len(),
        elapsed.as_millis(),
        parsed.prompt.as_ref(),
    );

    (ToolOutput::new(result), ToolResultStatus::Success)
}

// ─── WebFetch helpers ───────────────────────────────────────────

/// Validate a URL for fetching — scheme, length, no credentials, valid hostname.
fn validate_fetch_url(url: &str) -> std::result::Result<(), AppError> {
    if url.len() > MaxUrlLength::DEFAULT.value() {
        return Err(AppError::InvalidUrl {
            message: format!(
                "URL too long ({} chars, max {})",
                url.len(),
                MaxUrlLength::DEFAULT.value()
            ),
        });
    }

    let parsed = url::Url::parse(url).map_err(|e| AppError::InvalidUrl {
        message: format!("Invalid URL \"{url}\": {e}"),
    })?;

    match parsed.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(AppError::InvalidUrl {
                message: format!("Unsupported URL scheme: {scheme}"),
            });
        }
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(AppError::InvalidUrl {
            message: "URLs with credentials are not supported".into(),
        });
    }

    let host = parsed.host_str().unwrap_or("");
    if !host.contains('.') {
        return Err(AppError::InvalidUrl {
            message: format!("Invalid hostname: {host}"),
        });
    }

    Ok(())
}

/// Upgrade `http://` URLs to `https://`.
fn upgrade_to_https(url: &str) -> String {
    url.strip_prefix("http://")
        .map_or_else(|| url.to_string(), |rest| format!("https://{rest}"))
}

/// Result of a fetch — success or a cross-host redirect for the model to handle.
enum FetchResult {
    Success(reqwest::Response),
    CrossHostRedirect {
        original: String,
        redirect: String,
        status: u16,
    },
}

/// Fetch a URL following only same-host redirects (max 10 hops).
/// Cross-host redirects are returned as `FetchResult::CrossHostRedirect`.
async fn fetch_with_redirect_policy(url: &str) -> std::result::Result<FetchResult, AppError> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(
            FetchTimeoutSecs::DEFAULT.as_secs(),
        ))
        .user_agent("ClaudeCode/1.0")
        .build()
        .map_err(|e| AppError::HttpError {
            message: format!("Failed to create HTTP client: {e}"),
        })?;

    let mut current_url = url.to_string();
    let max_redirects = 10u8;

    for _ in 0..max_redirects {
        let resp = client
            .get(&current_url)
            .header("Accept", "text/markdown, text/html, */*")
            .send()
            .await
            .map_err(|e| AppError::HttpError {
                message: format!("HTTP request failed: {e}"),
            })?;

        if !resp.status().is_redirection() {
            return Ok(FetchResult::Success(resp));
        }

        let location = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or(AppError::HttpError {
                message: "Redirect missing Location header".into(),
            })?;

        let redirect_url = url::Url::parse(location)
            .or_else(|_| url::Url::parse(&current_url).and_then(|base| base.join(location)))
            .map_err(|e| AppError::HttpError {
                message: format!("Invalid redirect URL: {e}"),
            })?
            .to_string();

        if is_same_host(&current_url, &redirect_url) {
            current_url = redirect_url;
        } else {
            return Ok(FetchResult::CrossHostRedirect {
                original: current_url,
                redirect: redirect_url,
                status: resp.status().as_u16(),
            });
        }
    }

    Err(AppError::HttpError {
        message: format!("Too many redirects (exceeded {max_redirects})"),
    })
}

/// Check if two URLs share the same host (ignoring `www.` prefix).
fn is_same_host(a: &str, b: &str) -> bool {
    let strip_www = |h: &str| h.strip_prefix("www.").unwrap_or(h).to_lowercase();
    let host_a = url::Url::parse(a)
        .ok()
        .and_then(|u| u.host_str().map(&strip_www));
    let host_b = url::Url::parse(b)
        .ok()
        .and_then(|u| u.host_str().map(strip_www));
    host_a.is_some() && host_a == host_b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WebFetchInput;

    // ── WebFetch helpers ──

    #[test]
    fn validate_url_valid_https() {
        assert!(validate_fetch_url("https://example.com/page").is_ok());
    }

    #[test]
    fn validate_url_rejects_ftp() {
        let err = validate_fetch_url("ftp://example.com").unwrap_err();
        assert!(err.to_string().contains("Unsupported URL scheme"));
    }

    #[test]
    fn validate_url_rejects_credentials() {
        let err = validate_fetch_url("https://user:pass@example.com").unwrap_err();
        assert!(err.to_string().contains("credentials"));
    }

    #[test]
    fn validate_url_rejects_no_dot_hostname() {
        let err = validate_fetch_url("https://localhost/path").unwrap_err();
        assert!(err.to_string().contains("Invalid hostname"));
    }

    #[test]
    fn validate_url_rejects_too_long() {
        let long_url = format!("https://example.com/{}", "a".repeat(2000));
        let err = validate_fetch_url(&long_url).unwrap_err();
        assert!(err.to_string().contains("too long"));
    }

    #[test]
    fn upgrade_http_to_https() {
        assert_eq!(
            upgrade_to_https("http://example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn upgrade_keeps_https() {
        assert_eq!(
            upgrade_to_https("https://example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn same_host_ignores_www() {
        assert!(is_same_host(
            "https://example.com",
            "https://www.example.com"
        ));
        assert!(is_same_host(
            "https://www.example.com",
            "https://example.com"
        ));
    }

    #[test]
    fn same_host_different_hosts() {
        assert!(!is_same_host("https://example.com", "https://other.com"));
    }

    #[test]
    fn same_host_case_insensitive() {
        assert!(is_same_host("https://Example.COM", "https://example.com"));
    }

    #[test]
    fn webfetch_input_parses() {
        let json = serde_json::json!({
            "url": "https://example.com",
            "prompt": "summarize this"
        });
        let input: WebFetchInput = serde_json::from_value(json).unwrap();
        assert_eq!(input.url.as_ref(), "https://example.com");
        assert_eq!(input.prompt.as_ref(), "summarize this");
    }
}
